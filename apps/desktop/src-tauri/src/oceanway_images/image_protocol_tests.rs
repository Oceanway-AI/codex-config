use super::*;

fn profiles() -> Value {
    serde_json::from_str(include_str!("test-harness/image-profiles-current.json")).unwrap()
}

fn input(model: &str, base: &str, editing: bool) -> Input {
    let mut bytes=Cursor::new(Vec::new());
    DynamicImage::new_rgb8(2,3).write_to(&mut bytes,ImageFormat::Png).unwrap();
    Input {
        request: ImageTestRequest {model:model.into(),prompt:"A blue boat beside a red lighthouse.".into(),
            prompts:vec![],count:2,reference_paths:vec![],reference_urls:vec![],size:"1024x1024".into()},
        provider:SavedProvider {home:PathBuf::new(),config:String::new(),api_base:base.into(),key:"gateway-fixture-key".into(),
            default_model:None,allowed_models:vec![model.into()],local_mock:true},
        references:if editing {vec![Reference {bytes:bytes.into_inner().into(),mime:"image/png",extension:"png"}]} else {vec![]},
        directory:PathBuf::new(),protocol:ImageProtocol::OpenAi,
    }
}

#[test]
fn all_current_profiles_prepare_generation_and_published_local_edits() {
    let profiles=profiles();
    let mut generated=0;
    let mut edited=0;
    for (model,profile) in profiles["models"].as_object().unwrap() {
        for base in ["https://console.oceanway.tech/v1","https://customer.example/gateway/v1"] {
            for editing in [false,true] {
                let input=input(model,base,editing);
                let prepared=prepare(&input,1,profile);
                if editing && model.starts_with("grok-") {
                    assert!(prepared.is_err(),"must not silently turn an edit into text-to-image");
                    continue;
                }
                let prepared=prepared.unwrap_or_else(|e|panic!("{base} {model} edit={editing}: {e}"));
                assert!(prepared.body.as_object().unwrap().keys().all(|key|!key.starts_with('/')));
                assert!(prepared.body.get("async").is_none());
                assert!(prepared.query_base.starts_with(base.strip_suffix("/v1").unwrap()));
                let prefix=Url::parse(base).unwrap().path().trim_end_matches("/v1").to_owned();
                if model.starts_with("gemini-") {
                    assert_eq!(prepared.path,format!("{prefix}/v1beta/models/{model}:generateContent"));
                    assert_eq!(prepared.body.pointer("/generationConfig/candidateCount"),Some(&json!(1)));
                    assert!(prepared.body.get("n").is_none());
                    assert!(prepared.multipart_field.is_none());
                    if editing {
                        let data=prepared.body.pointer("/contents/0/parts/1/inlineData/data").unwrap().as_str().unwrap();
                        assert_eq!(STANDARD.decode(data).unwrap(),&*input.references[0].bytes);
                    }
                } else {
                    assert_eq!(prepared.body["model"],*model);
                    assert_eq!(prepared.body["n"],1); // count=2 is two queue items
                    assert_eq!(prepared.path,format!("{prefix}/v1/images/{}",if editing{"edits"}else{"generations"}));
                    assert_eq!(prepared.multipart_field.is_some(),editing);
                    if model.starts_with("grok-") {
                        assert!(prepared.body.get("size").is_none());
                        assert_eq!(prepared.body["resolution"],"1k");
                        assert_eq!(prepared.body["aspect_ratio"],"1:1");
                    } else {assert!(prepared.body.get("response_format").is_none());}
                }
                if editing {edited+=1;} else {generated+=1;}
            }
        }
    }
    assert_eq!((generated,edited),(20,16));
}

#[test]
fn published_requests_send_real_bytes_to_the_saved_gateway_with_its_key() {
    let profiles=profiles();
    for model in ["gpt-image-2","gemini-3-pro-image-preview","grok-imagine-image"] {
        for editing in [false,true] {
            if editing && model.starts_with("grok-") {continue;}
            let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base=format!("http://{}/customer/v1",listener.local_addr().unwrap());
            let mut input=input(model,&base,editing);
            input.protocol=ImageProtocol::Published(Arc::new(profiles["models"][model].clone()));
            let worker=thread::spawn(move || {
                let (mut stream,_)=listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut headers=Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {let mut byte=[0];stream.read_exact(&mut byte).unwrap();headers.push(byte[0]);}
                let headers=String::from_utf8(headers).unwrap().to_lowercase();
                let len:usize=headers.lines().find_map(|s|s.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                let mut body=vec![0;len]; stream.read_exact(&mut body).unwrap();
                let reply=r#"{"data":[{"b64_json":"wire-test"}]}"#;
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",reply.len()).unwrap();
                (headers,body)
            });
            submit(&input,1,Instant::now(),||Ok(input.provider.clone())).map_err(|e|e.message).unwrap();
            let (headers,body)=worker.join().unwrap();
            assert!(headers.contains("authorization: bearer gateway-fixture-key"));
            if model.starts_with("gpt-") && editing {
                assert!(headers.starts_with("post /customer/v1/images/edits "));
                assert!(headers.contains("multipart/form-data; boundary="));
                let fields=String::from_utf8_lossy(&body);
                assert!(fields.contains("name=\"image[]\"; filename=\"reference-1.png\""));
                assert!(fields.contains("name=\"n\"\r\n\r\n1"));
                assert!(!fields.contains("name=\"/n\""));
                assert!(body.windows(input.references[0].bytes.len()).any(|s|s==&*input.references[0].bytes));
            } else {
                assert!(headers.contains("content-type: application/json"));
                let body:Value=serde_json::from_slice(&body).unwrap();
                if model.starts_with("gemini-") {
                    assert!(headers.starts_with("post /customer/v1beta/models/"));
                    assert_eq!(body["generationConfig"]["candidateCount"],1);
                } else {assert_eq!(body["n"],1);}
            }
        }
    }
}

#[test]
fn pointers_limits_and_gateway_aliases_are_not_confused_with_request_fields() {
    let mut body=json!({});
    assign(&mut body,"/generationConfig/candidateCount",json!(1)).unwrap();
    assert_eq!(body,json!({"generationConfig":{"candidateCount":1}}));
    let profile=profiles()["models"]["gpt-image-2"].clone();
    let mut input=input("gpt-image-2","https://example.com/api/v1",true);
    input.references=vec![input.references[0].clone();17];
    assert!(prepare(&input,1,&profile).is_err());
    input.references.clear();input.request.size="32x32".into();
    assert!(prepare(&input,1,&profile).is_err());
    assert_eq!(catalog_protocol(&json!({"supported_endpoint_types":["gemini","image-generation"]})),ImageProtocol::OpenAi);
    assert_eq!(catalog_protocol(&json!({"supported_endpoint_types":["gemini","openai"]})),ImageProtocol::Gemini);
    assert!(!canonical_family("customer-painting-alias"));
    assert!(endpoint("https://example.com/v1","https://upstream.example/v1/images/generations","m").is_err());
}

#[test]
fn receipts_accept_current_location_and_preserve_legacy_queries_without_cross_host_keys() {
    let base="https://customer.example/prefix/v1";
    for path in ["/prefix/v1/tasks/t1","/prefix/v1/images/t1","/prefix/v1beta/tasks/t1"] {
        let url=async_images::validate_task_url(base,"t1",path).unwrap();
        let task=async_images::RemoteTask {id:"t1".into(),api_base:base.into(),query_base:base.into(),query_url:Some(url),credential_sha256:hash(b"test")};
        async_images::validate_receipt(&task).unwrap();
    }
    for path in ["https://console.oceanway.tech/v1/tasks/t1","/v1/tasks/t1","/prefix/v1/tasks/t2","/prefix/v1/tasks/t1?key=x"] {
        assert!(async_images::validate_task_url(base,"t1",path).is_err());
    }
    let task:async_images::RemoteTask=serde_json::from_value(json!({"id":"old-task","apiBase":base,"queryBase":base,"credentialSha256":hash(b"test")})).unwrap();
    assert!(task.query_url.is_none());
    async_images::validate_receipt(&task).unwrap();
}
