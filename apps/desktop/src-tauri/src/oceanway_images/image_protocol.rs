//! Protocol selection follows docs.oceanway.tech/docs/images and the selected
//! model's published media_generation_profile or gateway endpoint metadata.
use super::*;

struct Prepared { path: String, query_base: String, body: Value }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImageProtocol { Console, OpenAi, Gemini }

fn catalog_protocol(model: &Value) -> ImageProtocol {
    let supports = |name: &str| ["supported_endpoint_types", "supportedProtocols", "supported_protocols"]
        .iter().any(|field|model[*field].as_array().is_some_and(|values|values.iter().any(|value|value==name)));
    // An explicitly published Images endpoint also supports model aliases.
    if supports("image-generation") || supports("openai-images") { ImageProtocol::OpenAi }
    else if supports("gemini") { ImageProtocol::Gemini }
    else { ImageProtocol::OpenAi }
}

pub(super) fn discover(provider: &SavedProvider, model: &str) -> Result<ImageProtocol,String> {
    if Url::parse(&provider.api_base).ok().is_some_and(|url|super::super::provider::is_oceanway_endpoint(&url)) {
        return Ok(ImageProtocol::Console);
    }
    let response = provider_client(provider).map_err(|f|f.message)?
        .get(format!("{}/models",provider.api_base)).bearer_auth(&provider.key)
        .timeout(Duration::from_secs(25)).send()
        .map_err(|_|"Could not load image model capabilities. No image request was sent.")?;
    // Some standard Images services do not expose a model catalog.
    if matches!(response.status().as_u16(),404|405|501) { return Ok(ImageProtocol::OpenAi); }
    let (catalog,_) = json_response(response,&provider.key,4*1024*1024).map_err(|f|f.message)?;
    let rows = catalog["data"].as_array().ok_or("Invalid image model capabilities. No image request was sent.")?;
    let row = rows.iter().find(|row|row["id"]==model)
        .ok_or("The selected image model is no longer listed by this service. Refresh the image model list.")?;
    Ok(catalog_protocol(row))
}

pub(super) fn gemini_base(base: &str) -> Result<Url,String> {
    let mut url = Url::parse(base).map_err(|_|"Invalid Gemini image endpoint.")?;
    let path = url.path().trim_end_matches('/');
    let path = if path.ends_with("/v1beta") {path.to_owned()}
        else {format!("{}/v1beta",path.strip_suffix("/v1").unwrap_or(path))};
    url.set_path(&path);
    Ok(url)
}

fn prepare_custom_gemini(input: &Input,index: usize) -> Result<Prepared,String> {
    let request=&input.request;
    let prompt=if request.prompts.is_empty(){&request.prompt}else{request.prompts.get(index-1).ok_or("Missing image prompt.")?};
    let mut parts=vec![json!({"text":prompt})];
    for reference in &input.references {
        parts.push(json!({"inlineData":{"mimeType":reference.mime,"data":STANDARD.encode(&reference.bytes)}}));
    }
    for url in &request.reference_urls {parts.push(json!({"fileData":{"fileUri":url}}));}
    let mut body=json!({"contents":[{"role":"user","parts":parts}],
        "generationConfig":{"candidateCount":1,"responseModalities":["TEXT","IMAGE"]}});
    if !request.size.is_empty() && request.size!="auto" {
        let (width,height)=dimensions(&request.size).ok_or("Gemini image size must use WIDTHxHEIGHT or auto.")?;
        let ratios=[(1,1),(2,3),(3,2),(3,4),(4,3),(4,5),(5,4),(9,16),(16,9),(21,9)];
        let (w,h)=ratios.into_iter().find(|(w,h)|((width as f64/height as f64)-(*w as f64/ *h as f64)).abs()<0.002)
            .ok_or("The requested aspect ratio is not supported by Gemini images.")?;
        let resolution=match width.max(height) {1024=>"1K",2048=>"2K",4096=>"4K",_=>return Err("Gemini image resolution must be 1K, 2K or 4K; use auto for the model default.".into())};
        body["generationConfig"]["imageConfig"]=json!({"aspectRatio":format!("{w}:{h}"),"imageSize":resolution});
    }
    let mut endpoint=gemini_base(&input.provider.api_base)?;
    let query_base=endpoint.as_str().trim_end_matches('/').to_owned();
    endpoint.path_segments_mut().map_err(|_|"Invalid Gemini image endpoint.")?
        .push("models").push(&format!("{}:generateContent",request.model));
    Ok(Prepared {path:endpoint.path().into(),query_base,body})
}

pub(super) fn submit(input: &Input, index: usize, started: Instant,
    guard: impl FnOnce() -> Result<SavedProvider,Failure>) -> Result<(Value,Option<String>,SavedProvider,String),Failure> {
    if input.protocol == ImageProtocol::OpenAi {
        let mut submitted = None;
        let (body,id) = post_image_at_with_guard(input,index,started,|| {
            let provider = guard()?;
            submitted = Some(provider.clone());
            Ok(provider)
        })?;
        let provider = submitted.expect("admitted POST");
        return Ok((body,id,provider.clone(),provider.api_base));
    }
    if input.protocol == ImageProtocol::Gemini {
        let prepared=prepare_custom_gemini(input,index).map_err(Failure::not_sent)?;
        let client=provider_client(&input.provider)?;
        let provider=guard()?;
        let mut endpoint=Url::parse(&provider.api_base).map_err(|_|Failure::not_sent("Invalid saved image endpoint."))?;
        endpoint.set_path(&prepared.path);
        let response=client.post(endpoint).bearer_auth(&provider.key).json(&prepared.body)
            .timeout(remaining(started)?).send().map_err(|error|network_failure(&error))?;
        let (body,id)=json_response(response,&provider.key,JSON_BYTES)?;
        return Ok((body,id,provider,prepared.query_base));
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
    let total=request.reference_urls.len()+input.references.len();
    let native_edit=total>0 && variants.iter().any(|v|v["protocol"]=="openai-images" && v["operation"]=="edit");
    for variant in variants.iter().copied() {
        if variant["method"]!="POST" { continue; }
        let gemini=variant["protocol"]=="gemini";
        if !gemini && variant["protocol"]!="openai-images" { continue; }
        let editing=!gemini && native_edit;
        if variant["operation"]!=if editing {"edit"} else {"generate"} {continue;}
        let path=if gemini {
            if variant["path"]!="/v1beta/models/{model}:generateContent" { continue; }
            format!("/v1beta/models/{}:generateContent",request.model)
        } else {
            let suffix=if editing {"/images/edits"} else {"/images/generations"};
            if variant["path"]!=suffix { continue; }
            format!("/v1{suffix}")
        };
        let mut specs=variant["defaults"].as_object().cloned().unwrap_or_default();
        // Public image API delivery defaults to synchronous. The MCP job still
        // runs in its worker pool, so the Agent receives progress while it waits.
        specs.remove("async");
        let count_path=if gemini {"generationConfig.candidateCount"} else {"n"};
        let count_field=variant["parameter_bindings"].as_object()
            .and_then(|bindings|bindings.iter().find(|(_,path)|*path==count_path).map(|(name,_)|name.clone()))
            .ok_or("Image count parameter binding is missing.")?;
        specs.insert(count_field,json!(1));
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
        let mut compatible=true;
        if editing {
            let transport=references.and_then(|refs|refs.iter().find(|r|r["field"]=="images" && r["transport"]=="image-reference-array"));
            let Some(transport)=transport.filter(|r|total as u64<=r["max_items"].as_u64().unwrap_or(u64::MAX)) else {continue};
            if !reference_limits_allow(transport,&input.references) {continue;}
            let images:Vec<_>=input.references.iter().map(|r|json!({"image_url":format!("data:{};base64,{}",r.mime,STANDARD.encode(&r.bytes))}))
                .chain(request.reference_urls.iter().map(|url|json!({"image_url":url}))).collect();
            body["images"]=json!(images);
        } else if !request.reference_urls.is_empty() {
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
        if !editing && !input.references.is_empty() {
            let transport=references.and_then(|refs|refs.iter().find(|r|r["transport"]=="inline-data-array"));
            if let Some(transport)=transport.filter(|r|gemini && total as u64<=r["max_items"].as_u64().unwrap_or(u64::MAX)) {
                if !reference_limits_allow(transport,&input.references) {compatible=false;}
                for reference in &input.references {
                    body["contents"][0]["parts"].as_array_mut().unwrap().push(json!({"inlineData":{"mimeType":reference.mime,"data":STANDARD.encode(&reference.bytes)}}));
                }
            } else {compatible=false;}
        }
        if !compatible {continue;}
        let mut base=Url::parse(&input.provider.api_base).map_err(|_|"Invalid image provider.")?;
        if gemini {base.set_path("/v1beta");}
        return Ok(Prepared {path,query_base:base.to_string().trim_end_matches('/').into(),body});
    }
    if !input.references.is_empty() && variants.iter().all(|variant|
        !variant["native_reference_inputs"].as_array().is_some_and(|refs|
            refs.iter().any(|reference|reference["transport"]=="inline-data-array"
                || reference["transport"]=="image-reference-array"))) {
        return Err("This service currently publishes only URL references for this model. Local image attachments cannot be submitted with its published protocol. No image request was sent.".into());
    }
    Err("This endpoint does not publish a compatible size or reference input format for the selected model. No image request was sent.".into())
}

fn reference_limits_allow(transport: &Value, references: &[Reference]) -> bool {
    let total:usize=references.iter().map(|r|r.bytes.len()).sum();
    !transport["max_total_bytes"].as_u64().is_some_and(|max|total as u64>max)
        && references.iter().all(|reference|
            !transport["max_item_bytes"].as_u64().is_some_and(|max|reference.bytes.len() as u64>max)
            && transport["mime_types"].as_array().is_none_or(|types|types.iter().any(|m|m==reference.mime)))
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
    #[test]
    fn gateway_protocol_follows_capabilities_and_receipts_keep_the_path_prefix() {
        assert_eq!(catalog_protocol(&json!({"id":"any-alias","supported_endpoint_types":["gemini","openai"]})),ImageProtocol::Gemini);
        assert_eq!(catalog_protocol(&json!({"id":"gemini-named-alias","supported_endpoint_types":["gemini","image-generation"]})),ImageProtocol::OpenAi);
        assert_eq!(catalog_protocol(&json!({"id":"gemini-name-without-metadata"})),ImageProtocol::OpenAi);
        assert_eq!(catalog_protocol(&json!({"supportedProtocols":["gemini"]})),ImageProtocol::Gemini);
        assert_eq!(catalog_protocol(&json!({"supported_protocols":["openai-images"]})),ImageProtocol::OpenAi);
        for (base,expected) in [("https://example.com/custom/v1","https://example.com/custom/v1beta"),
            ("https://example.com/custom/v1beta","https://example.com/custom/v1beta"),
            ("https://example.com/custom/api","https://example.com/custom/api/v1beta")] {
            assert_eq!(gemini_base(base).unwrap().as_str(),expected);
            let mut task=async_images::RemoteTask {id:"task-fixture".into(),api_base:base.into(),query_base:expected.into(),credential_sha256:hash(b"fixture-key")};
            assert!(async_images::validate_receipt(&task).is_ok());
            task.query_base="https://example.com/v1beta".into();
            assert!(async_images::validate_receipt(&task).is_err());
        }
    }
    fn prepared(model: &str,size: &str,urls: Vec<String>) -> Result<Prepared,String> {
        prepared_with_local(model,size,urls,false)
    }
    fn prepared_with_local(model: &str,size: &str,urls: Vec<String>,local: bool) -> Result<Prepared,String> {
        let profiles:Value=serde_json::from_str(include_str!("test-harness/image-profiles.json")).unwrap();
        prepared_profile(model,size,urls,local,&profiles["models"][model])
    }
    fn prepared_profile(model: &str,size: &str,urls: Vec<String>,local: bool,profile: &Value) -> Result<Prepared,String> {
        let mut bytes=Cursor::new(Vec::new());
        DynamicImage::new_rgb8(1,1).write_to(&mut bytes,ImageFormat::Png).unwrap();
        let input=Input {request:ImageTestRequest {model:model.into(),prompt:"A quiet blue coast in soft morning light.".into(),
            prompts:vec![],count:1,reference_paths:vec![],reference_urls:urls,size:size.into()},
            provider:SavedProvider {home:PathBuf::new(),config:String::new(),api_base:"https://console.oceanway.tech/v1".into(),
                key:"fixture-only".into(),default_model:None,allowed_models:vec![model.into()],local_mock:false},
            references:if local {vec![Reference {bytes:bytes.into_inner().into(),mime:"image/png",extension:"png"}]} else {vec![]},directory:PathBuf::new(),protocol:ImageProtocol::Console};
        prepare(&input,1,profile)
    }
    #[test]
    fn native_console_profile_preserves_local_and_url_edits_and_defaults_to_sync() {
        // Media PR #13, 3fb6f739: native generation/edit PNG variants.
        let profile:Value=serde_json::from_str(include_str!("test-harness/image-profile-native.json")).unwrap();
        let generate=prepared_profile("image-alias","1024x1024",vec![],false,&profile).unwrap();
        assert_eq!(generate.path,"/v1/images/generations");
        assert_eq!(generate.body["model"],"image-alias");
        assert_eq!(generate.body["n"],1);
        assert!(generate.body.get("async").is_none());
        assert!(generate.body.get("response_format").is_none());
        assert!(generate.body.get("images").is_none());
        let edit=prepared_profile("image-alias","1024x1024",vec!["https://example.com/reference.png".into()],true,&profile).unwrap();
        assert_eq!(edit.path,"/v1/images/edits");
        assert_eq!(edit.body["n"],1);
        assert_eq!(edit.body["images"].as_array().unwrap().len(),2);
        let encoded=edit.body["images"][0]["image_url"].as_str().unwrap().strip_prefix("data:image/png;base64,").unwrap();
        let bytes=STANDARD.decode(encoded).unwrap();
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(),1);
        assert_eq!(edit.body["images"][1]["image_url"],"https://example.com/reference.png");
        assert!(edit.body.get("reference_images").is_none());
        assert!(edit.body.get("async").is_none());
        assert!(edit.body.get("response_format").is_none());
        let mut limited=profile.clone();
        for variant in limited["variants"].as_array_mut().unwrap() {
            for reference in variant["native_reference_inputs"].as_array_mut().into_iter().flatten() {
                reference["max_total_bytes"]=json!(1);
            }
        }
        let limit_error=prepared_profile("image-alias","1024x1024",vec![],true,&limited).err().unwrap();
        assert!(limit_error.contains("compatible size or reference input format"));
    }
    #[test]
    fn native_gemini_profile_accepts_optional_byte_limits_but_enforces_declared_limits() {
        let profiles:Value=serde_json::from_str(include_str!("test-harness/image-profiles.json")).unwrap();
        let mut profile=profiles["models"]["gemini-3-pro-image-preview"].clone();
        for variant in profile["variants"].as_array_mut().unwrap() {
            for reference in variant["native_reference_inputs"].as_array_mut().unwrap() {
                reference.as_object_mut().unwrap().remove("max_item_bytes");
                reference.as_object_mut().unwrap().remove("max_total_bytes");
            }
        }
        let result=prepared_profile("gemini-alias","1024x1024",vec![],true,&profile).unwrap();
        assert!(result.body.get("async").is_none());
        assert!(result.body.pointer("/contents/0/parts/1/inlineData/data").is_some());
        for variant in profile["variants"].as_array_mut().unwrap() {
            for reference in variant["native_reference_inputs"].as_array_mut().unwrap() {
                reference["max_total_bytes"]=json!(1);
            }
        }
        assert!(prepared_profile("gemini-alias","1024x1024",vec![],true,&profile).is_err());
    }
    #[test]
    fn console_local_references_follow_declared_transport_without_falling_back_to_text() {
        for model in ["gpt-image-2","grok-imagine-image"] {
            let error=prepared_with_local(model,"1024x1024",vec![],true).err().unwrap();
            assert!(error.contains("only URL references"));
            assert!(error.contains("No image request was sent"));
        }
        let gemini=prepared_with_local("gemini-3-pro-image-preview","1024x1024",vec![],true).unwrap();
        assert!(gemini.body.get("async").is_none());
        assert_eq!(gemini.body.pointer("/contents/0/parts/1/inlineData/mimeType"),Some(&json!("image/png")));
        let raw=STANDARD.decode(gemini.body.pointer("/contents/0/parts/1/inlineData/data").and_then(Value::as_str).unwrap()).unwrap();
        assert!(image::load_from_memory(&raw).is_ok());
    }
    #[test]
    fn official_profiles_choose_openai_and_gemini_without_mixing_fields() {
        let openai=prepared("gpt-image-2","1024x1024",vec!["https://example.com/a.png".into(),"https://example.com/b.png".into()]).unwrap();
        assert_eq!(openai.path,"/v1/images/generations");
        assert!(openai.body.get("async").is_none());
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
