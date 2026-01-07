// Insecure Network Module
// This module contains intentional security vulnerabilities

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::fs;
use std::thread;

// SECURITY ISSUE: Hardcoded IP and credentials
const SERVER_IP: &str = "192.168.1.100";
const ADMIN_TOKEN: &str = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
const PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...";

// CODE SMELL: Complex function with too many parameters
#[allow(dead_code)]
pub fn create_connection(
    host: &str,
    port: u16,
    timeout: u64,
    retry: bool,
    ssl: bool,
    validate_cert: bool,
    username: &str,
    password: &str,
    token: &str,
    api_key: &str,
) -> Result<TcpStream, std::io::Error> {
    // SECURITY ISSUE: Accepting ssl=false and validate_cert=false
    if !ssl || !validate_cert {
        println!("WARNING: Insecure connection!");
    }
    
    TcpStream::connect(format!("{}:{}", host, port))
}

// SECURITY ISSUE: Unbounded resource allocation (DoS vulnerability)
#[allow(dead_code)]
pub fn process_requests(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // SECURITY ISSUE: No limit on thread spawning - resource exhaustion
                thread::spawn(|| {
                    handle_client(stream);
                });
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
}

// SECURITY ISSUE: No input validation or size limits
#[allow(dead_code)]
fn handle_client(mut stream: TcpStream) {
    let mut buffer = Vec::new();
    
    // SECURITY ISSUE: Reading unbounded data into memory
    stream.read_to_end(&mut buffer).unwrap();
    
    // SECURITY ISSUE: Deserializing untrusted data
    let data = String::from_utf8_lossy(&buffer);
    
    // CODE SMELL: Ignoring result
    stream.write_all(b"OK").ok();
}

// SECURITY ISSUE: Directory traversal vulnerability
#[allow(dead_code)]
pub fn serve_file(filename: &str) -> Result<Vec<u8>, std::io::Error> {
    // No path sanitization - vulnerable to ../../../etc/passwd
    let path = format!("./public/{}", filename);
    fs::read(path)
}

// CODE SMELL: Empty catch block
#[allow(dead_code)]
pub fn risky_operation() {
    match dangerous_call() {
        Ok(_) => println!("Success"),
        Err(_) => {} // CODE SMELL: Silently ignoring errors
    }
}

#[allow(dead_code)]
fn dangerous_call() -> Result<(), String> {
    Err("Something went wrong".to_string())
}

// SECURITY ISSUE: Integer overflow not checked
#[allow(dead_code)]
pub fn allocate_buffer(size: usize, multiplier: usize) -> Vec<u8> {
    // Could overflow if size * multiplier > usize::MAX
    let total_size = size * multiplier;
    vec![0; total_size]
}

// CODE SMELL: Mutable global state without synchronization
#[allow(dead_code)]
static mut REQUEST_COUNT: u64 = 0;

#[allow(dead_code)]
pub fn increment_requests() {
    unsafe {
        // SECURITY ISSUE: Race condition - not thread safe
        REQUEST_COUNT += 1;
    }
}

// SECURITY ISSUE: Weak random number generation for security purposes
#[allow(dead_code)]
pub fn generate_session_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    // SECURITY ISSUE: Using time as random seed is predictable
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    now ^ 0xDEADBEEF  // Predictable "random" value
}

// CODE SMELL: Too many nested blocks
#[allow(dead_code)]
pub fn complex_parser(input: &str) -> Option<String> {
    if !input.is_empty() {
        if input.len() > 10 {
            if input.starts_with("data:") {
                if let Some(pos) = input.find(':') {
                    let data = &input[pos + 1..];
                    if !data.is_empty() {
                        if data.len() < 1000 {
                            return Some(data.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

// SECURITY ISSUE: Use of unsafe without proper justification
#[allow(dead_code)]
pub unsafe fn transmute_data<T>(data: &[u8]) -> &T {
    // DANGEROUS: No size or alignment checking
    &*(data.as_ptr() as *const T)
}

// CODE SMELL: Duplicate code
#[allow(dead_code)]
pub fn validate_username(username: &str) -> bool {
    if username.len() < 3 {
        return false;
    }
    if username.len() > 20 {
        return false;
    }
    if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    true
}

#[allow(dead_code)]
pub fn validate_password(password: &str) -> bool {
    if password.len() < 3 {
        return false;
    }
    if password.len() > 20 {
        return false;
    }
    if !password.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    true
}
