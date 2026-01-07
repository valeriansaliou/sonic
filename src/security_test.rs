// Sonic Security Test Module
// This module intentionally contains code smells and security issues

use std::fs::File;
use std::io::Read;
use std::process::Command;
use std::path::Path;

// CODE SMELL: Unused imports
use std::collections::HashMap;
use std::sync::Mutex;

// SECURITY ISSUE: Hardcoded credentials
const API_KEY: &str = "sk_live_1234567890abcdef";
const DATABASE_PASSWORD: &str = "admin123";
const SECRET_TOKEN: &str = "my-secret-token-12345";

// CODE SMELL: Dead code
#[allow(dead_code)]
fn unused_function() {
    println!("This function is never called");
}

// SECURITY ISSUE: Command injection vulnerability
pub fn execute_user_command(user_input: &str) -> String {
    // Directly using user input in shell command
    let output = Command::new("sh")
        .arg("-c")
        .arg(user_input)  // DANGEROUS: No sanitization
        .output()
        .expect("failed to execute command");
    
    String::from_utf8_lossy(&output.stdout).to_string()
}

// SECURITY ISSUE: Path traversal vulnerability
pub fn read_user_file(filename: &str) -> Result<String, std::io::Error> {
    // No validation of path - could access any file
    let path = format!("/var/data/{}", filename);
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

// CODE SMELL: Inefficient string concatenation in loop
pub fn build_query(terms: &[String]) -> String {
    let mut query = String::new();
    for term in terms {
        query = query + term + " ";  // Inefficient: creates new String each iteration
    }
    query
}

// CODE SMELL: Unnecessary clones
pub fn process_data(data: &String) -> String {
    let cloned = data.clone();
    let another_clone = cloned.clone();
    another_clone.to_uppercase()
}

// SECURITY ISSUE: Unsafe deserialization
pub fn deserialize_untrusted_data(data: &[u8]) -> Option<Vec<String>> {
    // Using unsafe deserialization without validation
    unsafe {
        let ptr = data.as_ptr() as *const Vec<String>;
        Some((*ptr).clone())
    }
}

// CODE SMELL: Magic numbers
pub fn calculate_timeout() -> u64 {
    42 * 1000  // What does 42 mean here?
}

// CODE SMELL: Deep nesting and complexity
pub fn complex_nested_function(a: i32, b: i32, c: i32) -> i32 {
    if a > 0 {
        if b > 0 {
            if c > 0 {
                if a > b {
                    if b > c {
                        return a + b + c;
                    } else {
                        if a > c {
                            return a * 2;
                        } else {
                            return c * 2;
                        }
                    }
                } else {
                    return b + c;
                }
            } else {
                return a + b;
            }
        } else {
            return a;
        }
    } else {
        return 0;
    }
}

// SECURITY ISSUE: Weak cryptographic algorithm (MD5)
pub fn hash_password(password: &str) -> String {
    // MD5 is cryptographically broken
    format!("{:x}", md5::compute(password))
}

// CODE SMELL: Panic in library code
pub fn divide(a: i32, b: i32) -> i32 {
    if b == 0 {
        panic!("Division by zero!");  // Should return Result instead
    }
    a / b
}

// SECURITY ISSUE: SQL injection vulnerability (simulated)
pub fn build_sql_query(user_id: &str, table: &str) -> String {
    // Direct string concatenation with user input
    format!("SELECT * FROM {} WHERE id = '{}'", table, user_id)
}

// CODE SMELL: Unwrap without proper error handling
pub fn parse_number(input: &str) -> i32 {
    input.parse::<i32>().unwrap()  // Will panic on invalid input
}

// SECURITY ISSUE: Race condition with TOCTOU (Time-of-check to time-of-use)
pub fn check_and_read_file(path: &str) -> Result<String, std::io::Error> {
    // Check if file exists
    if Path::new(path).exists() {
        // TOCTOU: File could be deleted or modified between check and use
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"))
    }
}

// CODE SMELL: Comparing floating points with ==
pub fn compare_floats(a: f64, b: f64) -> bool {
    a == b  // Dangerous due to floating point precision
}

// CODE SMELL: Useless conversion
pub fn identity_conversion(value: String) -> String {
    value.into()
}

// SECURITY ISSUE: Unvalidated redirect
pub fn redirect_user(url: &str) -> String {
    format!("Location: {}", url)  // No validation - could redirect anywhere
}

// CODE SMELL: Boolean trap
pub fn send_email(to: &str, subject: &str, body: &str, true_or_false: bool, another_bool: bool) {
    // What do these booleans mean without context?
    println!("Sending email to {} with flags: {}, {}", to, true_or_false, another_bool);
}
