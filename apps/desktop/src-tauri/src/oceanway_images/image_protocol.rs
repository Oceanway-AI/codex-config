//! Protocol selection follows docs.oceanway.tech/docs/images and the selected
//! model's published media_generation_profile. No model-name-only endpoint swap.
use super::*;

struct Prepared { path: String, query_base: String, body: Value }

pub(super) fn submit(input: &Input, index: usize, started: Instant,
    guard: impl FnOnce() -> Result<SavedProvider,Failure>) -> Result<(Value,Option<String>,SavedProvider,String),Failure> {
    let is_console = Url::parse(&input.provider.api_base).ok()
        .is_some_and(|url|super::super::provider::is_oceanway_endpoint(&url));
    if !is_console {
        let mut submitted = None;
        let (body,id) = post_image_at_with_guard(input,index,started,|| {
            let provider = guard()?;
            submitted = Some(provider.clone());
            Ok(provider)
        })?;
        let provider = submitted.expect("admitted POST");
        return Ok((body,id,provider.clone(),provider.api_base));
    }
    let mut url = Url::parse(&input.provider.api_base).map_err(|_|Failure::not_sent("Invalid Oceanway endpoint."))?;
    url.set_path("/api/platform/v1/public/offering");
    let client = provider_client(&input.provider)?;
    let response = client.get(url).timeout(Duration::from_secs(25)).send()
        .map_err(|_|Failure::not_sent("Could not load the image model's current capabilities."))?;
    let (catalog,_) = json_response(response,&input.provider.key,16*1024*1024)
        .map_err(|f|Failure::not_sent(f.message))?;
    let model = catalog.pointer("/data/models").and_then(Value::as_array)
        .and_then(|models|models.iter().find(|m|m["model_name"]==input.request.model && m["invocation_status"]=="available"))
        .ok_or_else(||Failure::not_sent("The selected image model is not currently available."))?;
    let prepared = prepare(input,index,&model["media_generation_profile"]).map_err(Failure::not_sent)?;
    let provider = guard()?;
    let mut origin = Url::parse(&provider.api_base).map_err(|_|Failure::not_sent("Invalid saved image endpoint."))?;
    origin.set_path(&prepared.path);
    let response = client.post(origin).bearer_auth(&provider.key).json(&prepared.body)
        .timeout(remaining(started)?).send().map_err(|error|network_failure(&error))?;
    let (body,id) = json_response(response,&provider.key,JSON_BYTES)?;
    Ok((body,id,provider,prepared.query_base))
}

fn assign(body: &mut Value, path: &str, value: Value) -> Result<(),String> {
    let parts: Vec<_> = path.split('.').collect();
    if parts.is_empty() || parts.len()>5 || parts.iter().any(|v|v.is_empty() || !v.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'_')) {
        return Err("Invalid published image parameter binding.".into());
    }
    let mut cursor = body;
    for part in &parts[..parts.len()-1] {
        if cursor.get(*part).is_none() { cursor[*part]=json!({}); }
        cursor = cursor.get_mut(*part).ok_or("Invalid image parameter structure.")?;
    }
    cursor[parts[parts.len()-1]]=value;
    Ok(())
}

fn conforms(value: &Value, schema: &Value) -> bool {
    if let Some(expected)=schema.get("const") { if value!=expected { return false; } }
    if let Some(values)=schema["enum"].as_array() { if !values.contains(value) { return false; } }
    match schema["type"].as_str() {
        Some("string")=>value.is_string(), Some("boolean")=>value.is_boolean(),
        Some("array")=>value.is_array(), Some("object")=>value.is_object(),
        Some("integer")=>value.as_u64().is_some_and(|n|n>=schema["minimum"].as_u64().unwrap_or(0)
            && n<=schema["maximum"].as_u64().unwrap_or(u64::MAX)), _=>false,
    }
}

fn dimensions(size: &str) -> Option<(u32,u32)> {
    let (w,h)=size.split_once('x')?; Some((w.parse().ok()?,h.parse().ok()?))
}

fn prepare(input: &Input, index: usize, profile: &Value) -> Result<Prepared,String> {
    let request=&input.request;
    let prompt=if request.prompts.is_empty(){&request.prompt}else{request.prompts.get(index-1).ok_or("Missing image prompt.")?};
    let variants=profile["variants"].as_array().ok_or("Image model capabilities are missing.")?;
    let mut variants:Vec<_>=variants.iter().collect();
    variants.sort_by_key(|v|v["id"]!=profile["default_variant"]);
    for variant in variants {
        if variant["method"]!="POST" || variant["operation"]!="generate" { continue; }
        let gemini=variant["protocol"]=="gemini";
        if !gemini && variant["protocol"]!="openai-images" { continue; }
        let path=if gemini {
            if variant["path"]!="/v1beta/models/{model}:generateContent" { continue; }
            format!("/v1beta/models/{}:generateContent",request.model)
        } else {
            if variant["path"]!="/images/generations" { continue; }
            "/v1/images/generations".into()
        };
        let mut specs=variant["defaults"].as_object().cloned().unwrap_or_default();
        specs.insert("images".into(),json!(1));
        if request.size!="auto" {
            if gemini {
                let Some((width,height))=dimensions(&request.size) else {continue};
                let ratio=variant["specifications_schema"]["properties"]["aspect_ratio"]["enum"].as_array()
                    .and_then(|values|values.iter().find(|v|v.as_str().and_then(|s|s.split_once(':'))
                        .and_then(|(w,h)|Some((w.parse::<f64>().ok()?,h.parse::<f64>().ok()?)))
                        .is_some_and(|(w,h)|((width as f64/height as f64)-(w/h)).abs()<0.002)));
                let Some(ratio)=ratio else {continue};
                let resolution=match width.max(height) {1024=>"1K",2048=>"2K",4096=>"4K",_=>continue};
                specs.insert("aspect_ratio".into(),ratio.clone());
                specs.insert("image_size".into(),json!(resolution));
            } else { specs.insert("size".into(),json!(request.size)); }
        }
        if specs.iter().any(|(key,value)|!conforms(value,&variant["specifications_schema"]["properties"][key])) { continue; }
        let mut body=if gemini {json!({"contents":[{"role":"user","parts":[{"text":prompt}]}]})}
            else {json!({"model":request.model,"prompt":prompt})};
        for (key,value) in &specs {
            assign(&mut body,variant["parameter_bindings"][key].as_str().ok_or("Image parameter binding is missing.")?,value.clone())?;
        }
        let references=variant["native_reference_inputs"].as_array();
        let total=request.reference_urls.len()+input.references.len();
        let mut compatible=true;
        if !request.reference_urls.is_empty() {
            let transport=references.and_then(|refs|refs.iter().find(|r|r["transport"]=="url-array"));
            if let Some(transport)=transport.filter(|r|total as u64<=r["max_items"].as_u64().unwrap_or(0)) {
                if gemini && transport["field"]=="contents.parts.fileData" {
                    for url in &request.reference_urls {
                        body["contents"][0]["parts"].as_array_mut().unwrap().push(json!({"fileData":{"fileUri":url}}));
                    }
                } else if !gemini && transport["field"]=="reference_images" {
                    body["reference_images"]=json!(request.reference_urls);
                } else {compatible=false;}
            } else {compatible=false;}
        }
        if !input.references.is_empty() {
            let transport=references.and_then(|refs|refs.iter().find(|r|r["transport"]=="inline-data-array"));
            if let Some(transport)=transport.filter(|r|gemini && total as u64<=r["max_items"].as_u64().unwrap_or(0)) {
                let sum:usize=input.references.iter().map(|r|r.bytes.len()).sum();
                if sum as u64>transport["max_total_bytes"].as_u64().unwrap_or(0) {compatible=false;}
                for reference in &input.references {
                    if reference.bytes.len() as u64>transport["max_item_bytes"].as_u64().unwrap_or(0)
                        || !transport["mime_types"].as_array().is_some_and(|types|types.iter().any(|m|m==reference.mime)) {compatible=false;}
                    body["contents"][0]["parts"].as_array_mut().unwrap().push(json!({"inlineData":{"mimeType":reference.mime,"data":STANDARD.encode(&reference.bytes)}}));
                }
            } else {compatible=false;}
        }
        if !compatible {continue;}
        let mut base=Url::parse(&input.provider.api_base).map_err(|_|"Invalid image provider.")?;
        if gemini {base.set_path("/v1beta");}
        return Ok(Prepared {path,query_base:base.to_string().trim_end_matches('/').into(),body});
    }
    Err("The requested size or references do not match this model's published capabilities. OpenAI images use supported WIDTHxHEIGHT; Gemini uses 1K/2K/4K with a supported ratio. Local file upload is pending for URL-only models; use original HTTPS reference_urls.".into())
}

pub(super) fn normalize_sync(body: Value) -> Value {
    if body.get("data").is_some() {return body;}
    let data:Vec<_>=body["candidates"].as_array().into_iter().flatten()
        .flat_map(|candidate|candidate.pointer("/content/parts").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|part|part.pointer("/inlineData/data").or_else(||part.pointer("/inline_data/data")))
        .map(|value|json!({"b64_json":value})).collect();
    if data.is_empty(){body}else{json!({"data":data})}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepared(model: &str,size: &str,urls: Vec<String>) -> Result<Prepared,String> {
        let profiles:Value=serde_json::from_str(include_str!("test-harness/image-profiles.json")).unwrap();
        let input=Input {request:ImageTestRequest {model:model.into(),prompt:"A quiet blue coast in soft morning light.".into(),
            prompts:vec![],count:1,reference_paths:vec![],reference_urls:urls,size:size.into()},
            provider:SavedProvider {home:PathBuf::new(),config:String::new(),api_base:"https://console.oceanway.tech/v1".into(),
                key:"fixture-only".into(),default_model:None,allowed_models:vec![model.into()],local_mock:false},
            references:vec![],directory:PathBuf::new()};
        prepare(&input,1,&profiles["models"][model])
    }
    #[test]
    fn official_profiles_choose_openai_and_gemini_without_mixing_fields() {
        let openai=prepared("gpt-image-2","1024x1024",vec!["https://example.com/a.png".into(),"https://example.com/b.png".into()]).unwrap();
        assert_eq!(openai.path,"/v1/images/generations");
        assert_eq!(openai.body["async"],true);
        assert_eq!(openai.body["n"],1);
        assert_eq!(openai.body["size"],"1024x1024");
        assert_eq!(openai.body["reference_images"],json!(["https://example.com/a.png","https://example.com/b.png"]));
        assert!(openai.body.get("generationConfig").is_none());
        let gemini=prepared("gemini-3-pro-image-preview","2048x2048",vec!["https://example.com/ref.png".into()]).unwrap();
        assert_eq!(gemini.path,"/v1beta/models/gemini-3-pro-image-preview:generateContent");
        assert_eq!(gemini.query_base,"https://console.oceanway.tech/v1beta");
        assert_eq!(gemini.body.pointer("/generationConfig/imageConfig/imageSize"),Some(&json!("2K")));
        assert_eq!(gemini.body.pointer("/contents/0/parts/1/fileData/fileUri"),Some(&json!("https://example.com/ref.png")));
        assert!(gemini.body.get("size").is_none());
        assert!(gemini.body.get("n").is_none());
        let grok=prepared("grok-imagine-image","1024x1024",vec![]).unwrap();
        assert!(grok.body.get("output_format").is_none());
    }
    #[test]
    fn model_size_and_reference_limits_are_checked_before_post() {
        assert!(prepared("grok-imagine-image","1024x1024",vec!["https://example.com/1.png".into(),"https://example.com/2.png".into()]).is_err());
        assert!(prepared("gemini-3-pro-image-preview","1900x1000",vec![]).is_err());
        assert!(prepared("gpt-image-2","32x32",vec![]).is_err());
    }
}
