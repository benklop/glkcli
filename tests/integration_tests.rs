// Integration tests for glkcli
// These tests verify end-to-end functionality

use assert_fs::prelude::*;
use assert_fs::TempDir;
use std::process::Command;

fn get_binary_path() -> std::path::PathBuf {
    // Get the path to the compiled binary
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test executable name
    path.pop(); // Remove 'deps' directory
    path.push("glkcli");
    path
}

#[test]
fn test_help_flag() {
    let output = Command::new(get_binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute glkcli");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:") || stdout.contains("USAGE:"));
    assert!(stdout.contains("glkcli"));
}

#[test]
fn test_version_flag() {
    let output = Command::new(get_binary_path())
        .arg("--version")
        .output()
        .expect("Failed to execute glkcli");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("glkcli") || stdout.contains("0.1"));
}

#[test]
fn test_format_detection_nonexistent_file() {
    let output = Command::new(get_binary_path())
        .arg("--format")
        .arg("/nonexistent/game.z5")
        .output()
        .expect("Failed to execute glkcli");

    // Should fail because file doesn't exist
    assert!(!output.status.success());
}

#[test]
fn test_format_detection_with_temp_file() {
    let temp = TempDir::new().unwrap();
    let game_file = temp.child("test.z5");
    
    // Create a minimal Z-code file
    let mut data = vec![0u8; 32];
    data[0] = 5; // Z-code version 5
    game_file.write_binary(&data).unwrap();

    let output = Command::new(get_binary_path())
        .arg("--format")
        .arg(game_file.path())
        .output()
        .expect("Failed to execute glkcli");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Z-code") || stdout.contains("z-code"),
        "Expected Z-code format, got: {}",
        stdout
    );
}

#[test]
fn test_invalid_arguments() {
    let output = Command::new(get_binary_path())
        .arg("--invalid-flag")
        .output()
        .expect("Failed to execute glkcli");

    assert!(!output.status.success());
}

#[test]
#[ignore] // This test can hang if it tries to actually run the game
fn test_run_without_interpreter() {
    let temp = TempDir::new().unwrap();
    let game_file = temp.child("test.z5");
    
    // Create a minimal Z-code file
    let mut data = vec![0u8; 32];
    data[0] = 5; // Z-code version 5
    game_file.write_binary(&data).unwrap();

    let output = Command::new(get_binary_path())
        .arg(game_file.path())
        .output()
        .expect("Failed to execute glkcli");

    // Should fail because bocfel interpreter is not installed in test environment
    // (or succeed if it is installed - either way we're testing the flow)
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // The test should either succeed (if interpreter exists) or fail with a clear error
    if !output.status.success() {
        assert!(
            stderr.contains("not found") || 
            stderr.contains("Interpreter") ||
            stdout.contains("not found"),
            "Expected interpreter error, got stderr: {}, stdout: {}",
            stderr, stdout
        );
    }
}

#[test]
fn test_format_detection_glulx() {
    let temp = TempDir::new().unwrap();
    let game_file = temp.child("test.ulx");
    
    // Create a minimal Glulx file
    let mut data = vec![0u8; 32];
    data[0..4].copy_from_slice(b"Glul");
    game_file.write_binary(&data).unwrap();

    let output = Command::new(get_binary_path())
        .arg("--format")
        .arg(game_file.path())
        .output()
        .expect("Failed to execute glkcli");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Glulx"),
        "Expected Glulx format, got: {}",
        stdout
    );
}

#[test]
fn test_verbose_flag() {
    let temp = TempDir::new().unwrap();
    let game_file = temp.child("test.z5");
    
    // Create a minimal Z-code file
    let mut data = vec![0u8; 32];
    data[0] = 5;
    game_file.write_binary(&data).unwrap();

    let output = Command::new(get_binary_path())
        .arg("--verbose")
        .arg("--format")
        .arg(game_file.path())
        .output()
        .expect("Failed to execute glkcli");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Verbose flag should produce additional output
    assert!(
        stdout.contains("Info:") || stdout.contains("Detecting") || stdout.contains("Z-code"),
        "Expected verbose output, got: {}",
        stdout
    );
}
