//! Anonymous Console capability metadata. API credentials never enter this client.
use reqwest::{blocking::Client, Url};
use serde_json::Value;
use std::{collections::BTreeMap, io::Read, sync::{Mutex, OnceLock}, time::{Duration, Instant}};

const ROOT: &str = "https://console.oceanway.tech/api/platform/v1/public/model-catalog/";
const LIMIT: u64 = 4 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Cache {
    summaries: Option<(Instant, Vec<Value>)>,
    profiles: BTreeMap<String, (Value, Value)>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn client() -> Result<Client, String> {
    Client::builder().redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never()).connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25)).build()
        .map_err(|_| "Could not initialize image capability discovery.".into())
}

fn read(client: &Client, url: Url) -> Result<Value, String> {
    let response = client.get(url).send().map_err(|_| "Could not load current image capabilities.")?;
    if !response.status().is_success() {
        return Err(format!("Image capability discovery returned HTTP {}.", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response.take(LIMIT + 1).read_to_end(&mut bytes).map_err(|_| "Could not read image capabilities.")?;
    if bytes.len() as u64 > LIMIT { return Err("Image capability response is too large.".into()); }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid image capability response.")?;
    if value["success"] != true { return Err("Image capabilities are unavailable.".into()); }
    Ok(value["data"].clone())
}

fn summaries_from(client: &Client, root: &str) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let mut cursor = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let mut url = Url::parse(&format!("{root}summaries")).map_err(|_| "Invalid catalog endpoint.")?;
        url.query_pairs_mut().append_pair("model_type", "image").append_pair("limit", "30");
        if !cursor.is_empty() { url.query_pairs_mut().append_pair("cursor", &cursor); }
        let data = read(client, url)?;
        if data["state"] != "summary" { return Err("Invalid image summary response.".into()); }
        items.extend(data["items"].as_array().ok_or("Missing image summaries.")?.iter().cloned());
        if data["has_more"] == false { return Ok(items); }
        cursor = data["next_cursor"].as_str().filter(|v| !v.is_empty()).ok_or("Missing catalog cursor.")?.into();
        if !seen.insert(cursor.clone()) { return Err("Repeated image catalog cursor.".into()); }
    }
    Err("Image catalog has too many pages.".into())
}

fn load_summaries(cache: &mut Cache, client: &Client, force: bool) -> Result<Vec<Value>, String> {
    if !force {
        if let Some((time, items)) = &cache.summaries {
            if time.elapsed() < TTL { return Ok(items.clone()); }
        }
    }
    let items = summaries_from(client, ROOT)?;
    cache.profiles.retain(|name, (revision, _)| items.iter().any(|row|
        row["model_name"] == *name && versions(row) == *revision));
    cache.summaries = Some((Instant::now(), items.clone()));
    Ok(items)
}

fn versions(row: &Value) -> Value {
    serde_json::json!([row["price_version"], row["capability_revision"], row["connection_revision"]])
}

pub(super) fn image_summaries() -> Result<Vec<Value>, String> {
    let mut cache = CACHE.get_or_init(Default::default).lock().map_err(|_| "Image catalog cache unavailable.")?;
    load_summaries(&mut cache, &client()?, false)
}

fn detail_from(client: &Client, root: &str, summary: &Value) -> Result<Value, String> {
    let mut url = Url::parse(&format!("{root}detail")).map_err(|_| "Invalid catalog endpoint.")?;
    let model = summary["model_name"].as_str().ok_or("Missing image model ID.")?;
    url.query_pairs_mut().append_pair("model_name", model);
    for field in ["price_version", "capability_revision", "connection_revision"] {
        let value = summary.get(field).filter(|v| !v.is_null()).ok_or("Missing image capability revision.")?;
        let value = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
        url.query_pairs_mut().append_pair(&format!("expected_{field}"), &value);
    }
    let data = read(client, url)?;
    if data["state"] != "detail" || data["model_name"] != model || versions(&data) != versions(summary) {
        return Err("Image capabilities changed during discovery. Refresh the model list.".into());
    }
    let profile = &data["model"]["media_generation_profile"];
    if !profile["variants"].is_array() { return Err("Image request variants are missing.".into()); }
    Ok(profile.clone())
}

pub(super) fn model_profile(model: &str) -> Result<Option<Value>, String> {
    let mut cache = CACHE.get_or_init(Default::default).lock().map_err(|_| "Image catalog cache unavailable.")?;
    let client = client()?;
    // A concurrent publication may invalidate expected revisions. Refresh GETs
    // once; this discovery never submits or repeats a generation request.
    for attempt in 0..2 {
        let summaries = load_summaries(&mut cache, &client, attempt == 1)?;
        let Some(summary) = summaries.iter().find(|row| row["model_name"] == model) else { return Ok(None); };
        if summary["invocation_status"] != "available" { return Err("The image model is not currently available.".into()); }
        if let Some((_, profile)) = cache.profiles.get(model) { return Ok(Some(profile.clone())); }
        match detail_from(&client, ROOT, summary) {
            Ok(profile) => {
                cache.profiles.insert(model.into(), (versions(summary), profile.clone()));
                return Ok(Some(profile));
            }
            Err(error) if attempt == 1 => return Err(error),
            Err(_) => {}
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use serde_json::json;

    #[test]
    fn summaries_paginate_and_detail_is_revision_matched_without_credentials() {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let root=format!("http://{}/catalog/",listener.local_addr().unwrap());
        let summary=json!({"model_name":"model one","price_version":"p1","capability_revision":"c1","connection_revision":2});
        let expected=summary.clone();
        let worker=std::thread::spawn(move || {
            for index in 0..4 {
                let (mut stream,_)=listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut headers=Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {let mut byte=[0];stream.read_exact(&mut byte).unwrap();headers.push(byte[0]);}
                let headers=String::from_utf8(headers).unwrap().to_lowercase();
                assert!(!headers.contains("authorization:"));
                let path=headers.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                let url=Url::parse(&format!("http://fixture{path}")).unwrap();
                let params:BTreeMap<_,_>=url.query_pairs().into_owned().collect();
                let data=if index<2 {
                    assert_eq!(url.path(),"/catalog/summaries");
                    assert_eq!(params["model_type"],"image");
                    if index==1 {assert_eq!(params["cursor"],"page/2");}
                    json!({"state":"summary","items":if index==0{vec![summary.clone()]}else{vec![json!({"model_name":"second"})]},
                        "has_more":index==0,"next_cursor":if index==0{"page/2"}else{""}})
                } else {
                    assert_eq!(url.path(),"/catalog/detail");
                    assert_eq!(params["model_name"],"model one");
                    assert_eq!(params["expected_price_version"],"p1");
                    assert_eq!(params["expected_capability_revision"],"c1");
                    assert_eq!(params["expected_connection_revision"],"2");
                    json!({"state":"detail","model_name":"model one","price_version":"p1",
                        "capability_revision":if index==2{"c1"}else{"c2"},"connection_revision":2,
                        "model":{"media_generation_profile":{"variants":[]}}})
                };
                let body=json!({"success":true,"data":data}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        let client=client().unwrap();
        let summaries=summaries_from(&client,&root).unwrap();
        assert_eq!(summaries.len(),2);
        assert_eq!(detail_from(&client,&root,&expected).unwrap(),json!({"variants":[]}));
        assert!(detail_from(&client,&root,&expected).is_err());
        worker.join().unwrap();
    }
}
