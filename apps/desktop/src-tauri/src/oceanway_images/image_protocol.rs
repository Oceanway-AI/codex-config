//! Model capabilities describe the wire format; the saved gateway owns routing.
use super::*;
use super::super::catalog;

struct Prepared {
    path: String,
    query_base: String,
    body: Value,
    multipart_field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ImageProtocol { Published(Arc<Value>), OpenAi, Gemini }

fn supports(model: &Value, name: &str) -> bool {
    ["supported_endpoint_types", "supportedProtocols", "supported_protocols"].iter().any(|field|
        model[*field].as_array().is_some_and(|values|values.iter().any(|value|value==name)))
}

fn catalog_protocol(model: &Value) -> ImageProtocol {
    if supports(model,"image-generation") || supports(model,"openai-images") { ImageProtocol::OpenAi }
    else if supports(model,"gemini") { ImageProtocol::Gemini }
    else { ImageProtocol::OpenAi }
}

fn canonical_family(model: &str) -> bool {
    model.starts_with("gpt-image-") || model.starts_with("grok-imagine-image")
        || (model.starts_with("gemini-") && (model.contains("image") || model.contains("nano-banana")))
}

pub(super) fn discover(provider: &SavedProvider, model: &str) -> Result<ImageProtocol,String> {
    if Url::parse(&provider.api_base).ok().is_some_and(|url|super::super::provider::is_oceanway_endpoint(&url)) {
        return catalog::model_profile(model)?.map(|p|ImageProtocol::Published(Arc::new(p)))
            .ok_or_else(||"The selected image model is not currently published.".into());
    }
    let response = provider_client(provider).map_err(|f|f.message)?
        .get(format!("{}/models",provider.api_base)).bearer_auth(&provider.key)
        .timeout(Duration::from_secs(25)).send()
        .map_err(|_|"Could not load image model capabilities. No image request was sent.")?;
    if matches!(response.status().as_u16(),404|405|501) { return Ok(ImageProtocol::OpenAi); }
    let (models,_) = json_response(response,&provider.key,MODEL_BYTES).map_err(|f|f.message)?;
    let rows = models["data"].as_array().ok_or("Invalid image model capabilities.")?;
    let row = rows.iter().find(|row|row["id"]==model)
        .ok_or("The selected image model is no longer listed by this service. Refresh the image model list.")?;
    if row["media_generation_profile"]["variants"].as_array().is_some_and(|variants|
        variants.iter().any(|v|v["native_request_variant"].is_object())) {
        return Ok(ImageProtocol::Published(Arc::new(row["media_generation_profile"].clone())));
    }
    // Exact Console model IDs can use its anonymous contract through New API.
    // Aliases keep the gateway's endpoint metadata, and every POST stays there.
    if canonical_family(model) {
        if let Ok(Some(profile)) = catalog::model_profile(model) {
            let gemini = profile["variants"].as_array().is_some_and(|variants|variants.iter()
                .any(|v|v["native_request_variant"]["protocol"]=="gemini.image"));
            let only_images = (supports(row,"image-generation") || supports(row,"openai-images")) && !supports(row,"gemini");
            if !gemini || !only_images { return Ok(ImageProtocol::Published(Arc::new(profile))); }
        }
    }
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

fn endpoint(base: &str, path: &str, model: &str) -> Result<(String,String),String> {
    let mut url = Url::parse(base).map_err(|_|"Invalid image provider.")?;
    let query_base;
    match path {
        "/v1/images/generations" | "/v1/images/edits" => {
            query_base = base.trim_end_matches('/').to_owned();
            url.set_path(&format!("{}{}",url.path().trim_end_matches('/'),&path[3..]));
        }
        "/v1beta/models/{model}:generateContent" => {
            url=gemini_base(base)?;
            query_base=url.as_str().trim_end_matches('/').to_owned();
            url.path_segments_mut().map_err(|_|"Invalid Gemini image endpoint.")?
                .push("models").push(&format!("{model}:generateContent"));
        }
        _ => return Err("The published image endpoint is not supported.".into()),
    }
    Ok((url.path().into(),query_base))
}

// JSON Pointer names belong to metadata, never to request property names.
fn assign(body: &mut Value, pointer: &str, value: Value) -> Result<(),String> {
    let parts: Vec<_> = pointer.strip_prefix('/').ok_or("Invalid image parameter pointer.")?
        .split('/').map(|p|p.replace("~1","/").replace("~0","~")).collect();
    if parts.is_empty() || parts.len()>8 || parts.iter().any(|p|p.is_empty()) {
        return Err("Invalid image parameter pointer.".into());
    }
    let mut cursor=body;
    for part in &parts[..parts.len()-1] {
        let object=cursor.as_object_mut().ok_or("Invalid image parameter structure.")?;
        cursor=object.entry(part.clone()).or_insert_with(||json!({}));
    }
    cursor.as_object_mut().ok_or("Invalid image parameter structure.")?.insert(parts.last().unwrap().clone(),value);
    Ok(())
}

fn schema_at<'a>(schema: &'a Value, pointer: &str) -> &'a Value {
    let mut cursor=schema;
    for part in pointer.trim_start_matches('/').split('/') { cursor=&cursor["properties"][part]; }
    cursor
}

fn conforms(value: &Value, schema: &Value) -> bool {
    if let Some(expected)=schema.get("const") { if value!=expected { return false; } }
    if let Some(values)=schema["enum"].as_array() { if !values.contains(value) { return false; } }
    match schema["type"].as_str() {
        Some("string")=>value.is_string(), Some("boolean")=>value.is_boolean(),
        Some("array")=>value.is_array(), Some("object")=>value.is_object(),
        Some("integer")=>value.as_u64().is_some_and(|n|n>=schema["minimum"].as_u64().unwrap_or(0)
            && n<=schema["maximum"].as_u64().unwrap_or(u64::MAX)),
        _=>false,
    }
}

fn dimensions(size: &str) -> Option<(u32,u32)> {
    let (w,h)=size.split_once('x')?;
    let (w,h)=(w.parse().ok()?,h.parse().ok()?);
    (w>0 && h>0).then_some((w,h))
}

fn size_allowed(size: (u32,u32), constraint: &Value) -> bool {
    let (w,h)=(size.0 as u64,size.1 as u64);
    let multiple=constraint["multiple_of"].as_u64().unwrap_or(1).max(1);
    w%multiple==0 && h%multiple==0 &&
        [(w,"min_width","max_width"),(h,"min_height","max_height"),(w*h,"min_pixels","max_pixels")]
            .into_iter().all(|(n,min,max)|n>=constraint[min].as_u64().unwrap_or(0) && n<=constraint[max].as_u64().unwrap_or(u64::MAX))
        && (w as f64/h as f64)>=constraint["min_aspect_ratio"].as_f64().unwrap_or(0.)
        && (w as f64/h as f64)<=constraint["max_aspect_ratio"].as_f64().unwrap_or(f64::INFINITY)
}

fn apply_size(body: &mut Value, native: &Value, size: &str) -> Result<(),String> {
    if size.is_empty() || size=="auto" { return Ok(()); }
    let (w,h)=dimensions(size).ok_or("Image size must use WIDTHxHEIGHT or auto.")?;
    let schema=&native["request_schema"];
    if schema["properties"].get("size").is_some() {
        let constraints=&native["output_size"];
        if !conforms(&json!(size),&schema["properties"]["size"]) ||
            (!constraints.is_null() && !size_allowed((w,h),constraints)) {
            return Err("The requested image size is not supported by this model.".into());
        }
        body["size"]=json!(size);
        return Ok(());
    }
    // Exact presets may use model-specific dimensions and resolutions.
    if let Some(preset)=native["size_presets"].as_array().and_then(|presets|presets.iter()
        .find(|p|p["width"]==w && p["height"]==h)) {
        if let Some(params)=preset["parameters"].as_object() {
            for (pointer,value) in params { assign(body,pointer,value.clone())?; }
            return Ok(());
        }
    }
    let gemini=native["protocol"]=="gemini.image";
    let (ratio_path,resolution_path)=if gemini {("/generationConfig/imageConfig/aspectRatio","/generationConfig/imageConfig/imageSize")}
        else {("/aspect_ratio","/resolution")};
    let ratio=schema_at(schema,ratio_path)["enum"].as_array().and_then(|values|values.iter().find(|v|
        v.as_str().and_then(|s|s.split_once(':')).and_then(|(a,b)|Some((a.parse::<f64>().ok()?,b.parse::<f64>().ok()?)))
            .is_some_and(|(a,b)|((w as f64/h as f64)-(a/b)).abs()<0.002)))
        .ok_or("The requested aspect ratio is not supported by this image model.")?;
    let resolution=match w.max(h) {1024=>"1K",2048=>"2K",4096=>"4K",_=>return Err("Use a published image size or auto.".into())};
    let resolution=if gemini {resolution.to_owned()} else {resolution.to_ascii_lowercase()};
    if !conforms(&json!(resolution),schema_at(schema,resolution_path)) {return Err("The requested resolution is not supported by this image model.".into());}
    assign(body,ratio_path,ratio.clone())?;
    assign(body,resolution_path,json!(resolution))
}

fn reference_limits_allow(transport: &Value, references: &[Reference]) -> bool {
    let total:usize=references.iter().map(|r|r.bytes.len()).sum();
    references.len() as u64>=transport["min_items"].as_u64().unwrap_or(0)
        && references.len() as u64<=transport["max_items"].as_u64().unwrap_or(u64::MAX)
        && !transport["max_total_bytes"].as_u64().is_some_and(|max|total as u64>max)
        && references.iter().all(|reference|
            !transport["max_item_bytes"].as_u64().is_some_and(|max|reference.bytes.len() as u64>max)
            && transport["mime_types"].as_array().is_none_or(|types|types.iter().any(|m|m==reference.mime)))
}

fn parts(input: &Input, index: usize) -> Result<Vec<Value>,String> {
    let mut parts=vec![json!({"text":prompt(input,index)?})];
    for reference in &input.references {
        parts.push(json!({"inlineData":{"mimeType":reference.mime,"data":STANDARD.encode(&reference.bytes)}}));
    }
    Ok(parts)
}

fn prompt(input: &Input,index: usize) -> Result<&str,String> {
    if input.request.prompts.is_empty() {Ok(&input.request.prompt)}
    else {input.request.prompts.get(index-1).map(String::as_str).ok_or_else(||"Missing image prompt.".into())}
}

fn prepare(input: &Input, index: usize, profile: &Value) -> Result<Prepared,String> {
    let editing=!input.references.is_empty();
    let variants=profile["variants"].as_array().ok_or("Image request variants are missing.")?;
    let mut variants:Vec<_>=variants.iter().collect();
    variants.sort_by_key(|v|v["id"]!=profile["default_variant"]);
    let mut last_error="This model does not publish the requested image operation.".to_owned();
    for variant in variants {
        let native=&variant["native_request_variant"];
        let protocol=native["protocol"].as_str().unwrap_or("");
        let gemini=protocol=="gemini.image";
        if native["method"]!="POST" || native["operation"]!="create" ||
            (!gemini && protocol!=if editing {"openai.image.edit"} else {"openai.image.generate"}) {continue;}
        let result=(|| {
            let mut body=native["defaults"].as_object().cloned().map(Value::Object).unwrap_or_else(||json!({}));
            let count_path=if gemini {"/generationConfig/candidateCount"} else {"/n"};
            if !conforms(&json!(1),schema_at(&native["request_schema"],count_path)) {return Err("The image model does not allow one output.".into());}
            assign(&mut body,count_path,json!(1))?;
            let multipart_field=if editing {
                let reference=native["native_reference_inputs"].as_array().and_then(|refs|refs.iter().find(|r|
                    if gemini {r["pointer"]=="/contents" && r["transport"]=="content-array"}
                    else {r["pointer"]=="/image" && r["transport"]=="image-reference-array"}))
                    .ok_or("This model does not publish reference-image input.")?;
                if !reference_limits_allow(reference,&input.references) {return Err("Reference images exceed the model's published count, format or byte limits.".into());}
                (!gemini).then(||"image[]".to_owned())
            } else {None};
            let content_type=if multipart_field.is_some(){"multipart/form-data"}else{"application/json"};
            if !native["content_types"].as_array().is_some_and(|types|types.iter().any(|t|t==content_type)) {
                return Err("The model does not publish this image request encoding.".into());
            }
            if gemini {
                body["contents"]=json!([{"role":"user","parts":parts(input,index)?}]);
                assign(&mut body,"/generationConfig/responseModalities",json!(["TEXT","IMAGE"]))?;
            } else {
                body["model"]=json!(input.request.model);
                body["prompt"]=json!(prompt(input,index)?);
            }
            apply_size(&mut body,native,&input.request.size)?;
            let (path,query_base)=endpoint(&input.provider.api_base,native["path"].as_str().ok_or("Missing image endpoint.")?,&input.request.model)?;
            Ok(Prepared {path,query_base,body,multipart_field})
        })();
        match result {Ok(prepared)=>return Ok(prepared),Err(error)=>last_error=error}
    }
    Err(last_error)
}

fn materialize_references(input: &Input, started: Instant) -> Result<Input,Failure> {
    let mut input=input.clone();
    for url in &input.request.reference_urls {
        let bytes=download_image(url,started)?;
        let (_,format)=decode_image(&bytes)?;
        let (mime,extension)=format_info(format);
        input.references.push(Reference {bytes:bytes.into(),mime,extension});
    }
    input.request.reference_urls.clear();
    Ok(input)
}

fn prepare_custom_gemini(input: &Input,index: usize) -> Result<Prepared,String> {
    let mut body=json!({"contents":[{"role":"user","parts":parts(input,index)?}],
        "generationConfig":{"candidateCount":1,"responseModalities":["TEXT","IMAGE"]}});
    for url in &input.request.reference_urls {
        body["contents"][0]["parts"].as_array_mut().unwrap().push(json!({"fileData":{"fileUri":url}}));
    }
    let native=json!({"protocol":"gemini.image","request_schema":{"properties":{"generationConfig":{"properties":{"imageConfig":{"properties":{
        "aspectRatio":{"type":"string","enum":["1:1","2:3","3:2","3:4","4:3","4:5","5:4","9:16","16:9","21:9"]},
        "imageSize":{"type":"string","enum":["1K","2K","4K"]}}}}}}}});
    apply_size(&mut body,&native,&input.request.size)?;
    let (path,query_base)=endpoint(&input.provider.api_base,"/v1beta/models/{model}:generateContent",&input.request.model)?;
    Ok(Prepared {path,query_base,body,multipart_field:None})
}

fn request_builder(client: &Client,input: &Input,prepared: &Prepared) -> Result<reqwest::blocking::RequestBuilder,Failure> {
    let mut endpoint=Url::parse(&input.provider.api_base).map_err(|_|Failure::not_sent("Invalid image endpoint."))?;
    endpoint.set_path(&prepared.path);
    let builder=client.post(endpoint);
    if let Some(field)=&prepared.multipart_field {
        let mut form=multipart::Form::new();
        for (name,value) in prepared.body.as_object().ok_or_else(||Failure::not_sent("Invalid image request body."))? {
            form=form.text(name.clone(),value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string()));
        }
        for (index,reference) in input.references.iter().enumerate() {
            let part=multipart::Part::reader_with_length(Cursor::new(Arc::clone(&reference.bytes)),reference.bytes.len() as u64)
                .file_name(format!("reference-{}.{}",index+1,reference.extension)).mime_str(reference.mime)
                .map_err(|_|Failure::not_sent("Invalid reference image MIME type."))?;
            form=form.part(field.clone(),part);
        }
        Ok(builder.multipart(form))
    } else {Ok(builder.json(&prepared.body))}
}

pub(super) fn submit(input: &Input, index: usize, started: Instant,
    guard: impl FnOnce() -> Result<SavedProvider,Failure>) -> Result<(Value,Option<String>,SavedProvider,String),Failure> {
    if input.protocol == ImageProtocol::OpenAi {
        let mut submitted=None;
        let (body,id)=post_image_at_with_guard(input,index,started,|| {
            let provider=guard()?; submitted=Some(provider.clone()); Ok(provider)
        })?;
        let provider=submitted.expect("admitted POST");
        return Ok((body,id,provider.clone(),provider.api_base));
    }
    let input=if matches!(input.protocol,ImageProtocol::Published(_)) {materialize_references(input,started)?} else {input.clone()};
    let prepared=match &input.protocol {
        ImageProtocol::Published(profile)=>prepare(&input,index,profile),
        ImageProtocol::Gemini=>prepare_custom_gemini(&input,index),
        ImageProtocol::OpenAi=>unreachable!(),
    }.map_err(Failure::not_sent)?;
    let client=provider_client(&input.provider)?;
    let builder=request_builder(&client,&input,&prepared)?;
    let provider=guard()?;
    // The admission guard rejects endpoint/model changes. Key rotation may be
    // admitted; bind that current key only after building the credential-free body.
    let response=builder.bearer_auth(&provider.key).timeout(remaining(started)?).send()
        .map_err(|error|network_failure(&error))?;
    let (body,id)=json_response(response,&provider.key,JSON_BYTES)?;
    Ok((body,id,provider,prepared.query_base))
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
#[path = "image_protocol_tests.rs"]
mod tests;
