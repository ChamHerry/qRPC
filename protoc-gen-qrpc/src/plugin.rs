//! protoc plugin protocol handling.

use crate::{go_gen, python_gen, rust_gen};
use prost::Message;
use prost_types::compiler::{code_generator_response, CodeGeneratorRequest, CodeGeneratorResponse};
use prost_types::{FileDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto};
use std::collections::HashSet;
use std::io::{Read, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoFile {
    pub name: String,
    pub package: String,
    pub stem: String,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub full_name: String,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub name: String,
    pub rust_name: String,
    pub route: String,
    pub input_type: String,
    pub output_type: String,
    pub kind: MethodKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    Unary,
    ServerStream,
    ClientStream,
    BidiStream,
}

impl MethodKind {
    fn from_flags(client_streaming: bool, server_streaming: bool) -> Self {
        match (client_streaming, server_streaming) {
            (false, false) => Self::Unary,
            (false, true) => Self::ServerStream,
            (true, false) => Self::ClientStream,
            (true, true) => Self::BidiStream,
        }
    }
}

pub fn run<R: Read, W: Write>(mut input: R, mut output: W) -> Result<(), String> {
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .map_err(|err| format!("failed to read CodeGeneratorRequest: {err}"))?;
    let response = generate_from_bytes(&bytes)?;
    output
        .write_all(&response)
        .map_err(|err| format!("failed to write CodeGeneratorResponse: {err}"))
}

pub fn generate_from_bytes(input: &[u8]) -> Result<Vec<u8>, String> {
    let request = CodeGeneratorRequest::decode(input)
        .map_err(|err| format!("failed to decode CodeGeneratorRequest: {err}"))?;
    let response = generate(request);
    let mut output = Vec::new();
    response
        .encode(&mut output)
        .map_err(|err| format!("failed to encode CodeGeneratorResponse: {err}"))?;
    Ok(output)
}

pub fn generate(request: CodeGeneratorRequest) -> CodeGeneratorResponse {
    match build_models(&request) {
        Ok(files) => response_with_files(files),
        Err(error) => CodeGeneratorResponse {
            error: Some(error),
            supported_features: None,
            file: Vec::new(),
        },
    }
}

fn response_with_files(files: Vec<ProtoFile>) -> CodeGeneratorResponse {
    let mut response = CodeGeneratorResponse {
        error: None,
        supported_features: None,
        file: Vec::new(),
    };

    for file in files {
        if file.services.is_empty() {
            continue;
        }
        response.file.push(response_file(
            format!("{}.qrpc.rs", file.stem),
            rust_gen::generate(&file),
        ));
        response.file.push(response_file(
            format!("{}_qrpc.py", file.stem),
            python_gen::generate(&file),
        ));
        response.file.push(response_file(
            format!("{}_qrpc.go", file.stem),
            go_gen::generate(&file),
        ));
    }

    response
}

fn response_file(name: String, content: String) -> code_generator_response::File {
    code_generator_response::File {
        name: Some(name),
        insertion_point: None,
        content: Some(content),
        generated_code_info: None,
    }
}

fn build_models(request: &CodeGeneratorRequest) -> Result<Vec<ProtoFile>, String> {
    let targets = request
        .file_to_generate
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let mut files = Vec::new();

    for file in &request.proto_file {
        let Some(name) = &file.name else {
            continue;
        };
        if !targets.contains(name) {
            continue;
        }
        files.push(model_file(file)?);
    }

    Ok(files)
}

fn model_file(file: &FileDescriptorProto) -> Result<ProtoFile, String> {
    let name = file.name.clone().unwrap_or_default();
    let package = file.package.clone().unwrap_or_default();
    let stem = file_stem(&name)?;
    let mut services = Vec::new();
    for service in &file.service {
        services.push(model_service(&package, service)?);
    }
    Ok(ProtoFile {
        name,
        package,
        stem,
        services,
    })
}

fn model_service(package: &str, service: &ServiceDescriptorProto) -> Result<Service, String> {
    let name = required_name(service.name.as_deref(), "service")?;
    let full_name = if package.is_empty() {
        name.clone()
    } else {
        format!("{package}.{name}")
    };
    let mut methods = Vec::new();
    for method in &service.method {
        methods.push(model_method(&full_name, method)?);
    }
    Ok(Service {
        name,
        full_name,
        methods,
    })
}

fn model_method(full_service: &str, method: &MethodDescriptorProto) -> Result<Method, String> {
    let name = required_name(method.name.as_deref(), "method")?;
    let input_type = type_name(method.input_type.as_deref());
    let output_type = type_name(method.output_type.as_deref());
    let kind = MethodKind::from_flags(
        method.client_streaming.unwrap_or(false),
        method.server_streaming.unwrap_or(false),
    );
    Ok(Method {
        rust_name: snake_case(&name),
        route: format!("/{full_service}/{name}"),
        name,
        input_type,
        output_type,
        kind,
    })
}

fn required_name(value: Option<&str>, kind: &str) -> Result<String, String> {
    let Some(value) = value else {
        return Err(format!("{kind} is missing name"));
    };
    if value.is_empty() {
        return Err(format!("{kind} has empty name"));
    }
    Ok(value.to_string())
}

fn type_name(value: Option<&str>) -> String {
    value.unwrap_or("").trim_start_matches('.').to_string()
}

fn file_stem(name: &str) -> Result<String, String> {
    let file = name
        .rsplit('/')
        .next()
        .ok_or_else(|| "proto file name is empty".to_string())?;
    let stem = file.strip_suffix(".proto").unwrap_or(file);
    let stem = snake_case(stem);
    if stem.is_empty() {
        return Err(format!("invalid proto file name: {name}"));
    }
    Ok(stem)
}

pub fn snake_case(value: &str) -> String {
    let mut out = String::new();
    let mut prev_is_lower_or_digit = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() {
                if prev_is_lower_or_digit && !out.ends_with('_') {
                    out.push('_');
                }
                out.push(ch.to_ascii_lowercase());
                prev_is_lower_or_digit = false;
            } else {
                out.push(ch);
                prev_is_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
            }
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
            prev_is_lower_or_digit = false;
        }
    }
    out.trim_matches('_').to_string()
}

pub fn upper_camel(value: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                out.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                out.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use prost_types::compiler::CodeGeneratorRequest;
    use prost_types::{FileDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto};

    #[test]
    fn generates_three_language_files_for_service() {
        let response = generate(request_with_methods(vec![method("Unary", false, false)]));

        assert!(response.error.is_none());
        assert_eq!(response.file.len(), 3);
        assert_eq!(response.file[0].name.as_deref(), Some("echo.qrpc.rs"));
        assert_eq!(response.file[1].name.as_deref(), Some("echo_qrpc.py"));
        assert_eq!(response.file[2].name.as_deref(), Some("echo_qrpc.go"));
        let rust = response.file[0].content.as_deref().expect("rust");
        assert!(rust.contains("pub struct EchoClient"));
        assert!(rust.contains("pub async fn unary"));
        assert!(rust.contains("\"/demo.Echo/Unary\""));
    }

    #[test]
    fn encodes_and_decodes_protoc_plugin_response() {
        let request = request_with_methods(vec![method("Unary", false, false)]);
        let mut input = Vec::new();
        request.encode(&mut input).expect("encode request");

        let output = generate_from_bytes(&input).expect("generate bytes");
        let response = CodeGeneratorResponse::decode(output.as_slice()).expect("decode response");

        assert!(response.error.is_none());
        assert_eq!(response.file.len(), 3);
    }

    #[test]
    fn classifies_all_streaming_shapes() {
        let response = generate(request_with_methods(vec![
            method("Unary", false, false),
            method("Watch", false, true),
            method("Upload", true, false),
            method("Chat", true, true),
        ]));
        let rust = response.file[0].content.as_deref().expect("rust");

        assert!(rust.contains("pub async fn unary"));
        assert!(rust.contains("pub async fn watch"));
        assert!(rust.contains("pub async fn upload"));
        assert!(rust.contains("pub async fn chat"));
        assert!(rust.contains("start_call(\"/demo.Echo/Chat\")"));
    }

    fn request_with_methods(methods: Vec<MethodDescriptorProto>) -> CodeGeneratorRequest {
        CodeGeneratorRequest {
            file_to_generate: vec!["demo/echo.proto".to_string()],
            parameter: None,
            proto_file: vec![FileDescriptorProto {
                name: Some("demo/echo.proto".to_string()),
                package: Some("demo".to_string()),
                service: vec![ServiceDescriptorProto {
                    name: Some("Echo".to_string()),
                    method: methods,
                    options: None,
                }],
                ..Default::default()
            }],
            compiler_version: None,
        }
    }

    fn method(name: &str, client_streaming: bool, server_streaming: bool) -> MethodDescriptorProto {
        MethodDescriptorProto {
            name: Some(name.to_string()),
            input_type: Some(".demo.EchoRequest".to_string()),
            output_type: Some(".demo.EchoReply".to_string()),
            client_streaming: Some(client_streaming),
            server_streaming: Some(server_streaming),
            ..Default::default()
        }
    }
}
