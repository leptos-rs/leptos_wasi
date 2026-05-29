use reqwest::StatusCode;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

struct WasmtimeServer {
    child: Child,
    port: u16,
    stderr_log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Drop for WasmtimeServer {
    fn drop(&mut self) {
        println!("Stopping Wasmtime server on port {}", self.port);
        let _ = self.child.kill();
        let _ = self.child.wait();

        if std::thread::panicking()
            && let Ok(log) = self.stderr_log.lock()
        {
            eprintln!(
                "\n--- Wasmtime Server Stderr on port {} (test failed) ---",
                self.port
            );
            for line in log.iter() {
                eprintln!("{}", line);
            }
            eprintln!("--------------------------------------------------");
        }
    }
}

fn start_server(
    wasm_path: &str,
    is_p3: bool,
) -> anyhow::Result<WasmtimeServer> {
    let mut args =
        vec!["serve".to_string(), "-S".to_string(), "cli=y".to_string()];
    if is_p3 {
        args.push("-S".to_string());
        args.push("p3=y".to_string());
        args.push("-W".to_string());
        args.push("component-model-async=y".to_string());
    }

    // Mount the static files directory to the virtual filesystem in the guest.
    args.push("--dir".to_string());
    args.push("tests/test-app/static::/static".to_string());

    args.push(wasm_path.to_string());
    args.push("--addr".to_string());
    args.push("127.0.0.1:0".to_string());

    println!("Spawning wasmtime {:?}", args);
    let mut child = Command::new("wasmtime")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to take stderr"))?;
    let mut reader = BufReader::new(stderr);

    let mut port = None;
    let mut line = String::new();
    let stderr_log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stderr_log_clone = stderr_log.clone();

    for _ in 0..100 {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim().to_string();
        if let Ok(mut log) = stderr_log_clone.lock() {
            log.push(trimmed.clone());
        }
        println!("wasmtime output: {}", trimmed);
        if line.contains("Serving HTTP on") && line.rfind(':').is_some() {
            let pos = line.rfind(':').unwrap();
            let port_str = line[pos + 1..].trim().trim_end_matches('/');
            if let Ok(p) = port_str.parse::<u16>() {
                port = Some(p);
                break;
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow::anyhow!(
                "wasmtime serve exited early with status: {}",
                status
            ));
        }
    }

    let port = port.ok_or_else(|| {
        anyhow::anyhow!("Failed to parse port from wasmtime serve output")
    })?;

    // Spawn thread to drain the rest of stderr so wasmtime does not block on write
    std::thread::spawn(move || {
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            let trimmed = line.trim().to_string();
            if let Ok(mut log) = stderr_log_clone.lock() {
                log.push(trimmed);
            }
            line.clear();
        }
    });

    Ok(WasmtimeServer {
        child,
        port,
        stderr_log,
    })
}

async fn run_assertions(
    port: u16,
    test_static_files: bool,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none()) // Don't auto-follow redirects so we can test 302 locations
        .build()?;

    let base_url = format!("http://127.0.0.1:{}", port);
    println!("Running assertions against {}", base_url);

    // 1. GET /api/get_test
    {
        let res = client
            .get(format!("{}/api/get_test", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(
            text.contains("GET response"),
            "Expected 'GET response', got: {}",
            text
        );
    }

    // 2. POST /api/post_test
    {
        let res = client
            .post(format!("{}/api/post_test", base_url))
            .header("Content-Type", "application/json")
            .body(r#"{"msg": "hello"}"#)
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(
            text.contains("POST response: hello"),
            "Expected 'POST response: hello', got: {}",
            text
        );
    }

    // 3. POST /api/generic_test
    {
        let res = client
            .post(format!("{}/api/generic_test", base_url))
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(
            text.contains("Generic response"),
            "Expected 'Generic response', got: {}",
            text
        );
    }

    // 4. POST /api/custom_test
    {
        let res = client
            .post(format!("{}/api/custom_test", base_url))
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(
            text.contains("Custom response"),
            "Expected 'Custom response', got: {}",
            text
        );
    }

    // 5. POST /api/panic_test
    {
        let res = client
            .post(format!("{}/api/panic_test", base_url))
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await;
        if test_static_files {
            // Wasmtime returns 500 Internal Server Error
            let res = res?;
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        } else {
            // Spin may drop the connection or return a non-200 status
            match res {
                Ok(res) => assert_ne!(
                    res.status(),
                    StatusCode::OK,
                    "Panic endpoint should not return 200 OK"
                ),
                Err(_) => { /* connection dropped — acceptable for Spin */ }
            }
        }
    }

    // 6. POST /api/form_submit_test (with Referer)
    {
        let res = client
            .post(format!("{}/api/form_submit_test", base_url))
            .header("Accept", "text/html")
            .header("Referer", "http://127.0.0.1/previous-page")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::FOUND);
        let loc = res
            .headers()
            .get("Location")
            .expect("Location header missing");
        let loc_clean = loc.to_str()?.trim_end_matches('?');
        assert!(
            loc_clean == "/previous-page"
                || loc_clean.ends_with("/previous-page"),
            "Expected relative /previous-page or absolute ending with \
             /previous-page, got: {}",
            loc_clean
        );
    }

    // 7. POST /api/form_submit_test (with Referrer spelling)
    {
        let res = client
            .post(format!("{}/api/form_submit_test", base_url))
            .header("Accept", "text/html")
            .header("Referrer", "http://127.0.0.1/other-page")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::FOUND);
        let loc = res
            .headers()
            .get("Location")
            .expect("Location header missing");
        let loc_clean = loc.to_str()?.trim_end_matches('?');
        assert!(
            loc_clean == "/other-page" || loc_clean.ends_with("/other-page"),
            "Expected relative /other-page or absolute ending with \
             /other-page, got: {}",
            loc_clean
        );
    }

    // Adversarial Test 1: Open Redirect Vulnerability Prevention
    {
        let res = client
            .post(format!("{}/api/form_submit_test", base_url))
            .header("Accept", "text/html")
            .header("Referer", "https://malicious.example.com/steal-session")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::FOUND);
        let loc = res
            .headers()
            .get("Location")
            .expect("Location header missing");
        let loc_clean = loc.to_str()?.trim_end_matches('?');
        assert!(
            loc_clean == "/steal-session"
                || loc_clean.ends_with("/steal-session"),
            "Expected relative /steal-session or absolute ending with \
             /steal-session, got: {}",
            loc_clean
        );
    }

    // Adversarial Test 1b: Open Redirect Backslash Bypass Prevention
    {
        let backslash_referrers = vec![
            "http://127.0.0.1/\\\\evil.com/steal-session",
            "http://127.0.0.1/%5C%5Cevil.com/steal-session",
        ];
        for ref_url in backslash_referrers {
            let res = client
                .post(format!("{}/api/form_submit_test", base_url))
                .header("Accept", "text/html")
                .header("Referer", ref_url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .send()
                .await?;
            assert_eq!(res.status(), StatusCode::FOUND);
            let loc = res
                .headers()
                .get("Location")
                .expect("Location header missing");
            let loc_clean = loc.to_str()?.trim_end_matches('?');
            assert!(
                loc_clean == "/"
                    || loc_clean.ends_with('/')
                    || (!loc_clean.contains("evil.com")
                        && !loc_clean.contains('\\')),
                "Expected location to fall back to '/' or at least not \
                 contain evil.com or backslashes, got: {}",
                loc_clean
            );
        }
    }

    // Adversarial Test 2: Process Panic on Malformed Header Redirects Prevention
    {
        let res = client
            .post(format!("{}/api/malformed_redirect_test", base_url))
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // Adversarial Test 3: Overlapping Prefix Route Hijacking Prevention
    {
        let res = client
            .get(format!("{}/static-page", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(
            text.contains("Static Page SSR View"),
            "Expected 'Static Page SSR View', got: {}",
            text
        );
    }

    // Adversarial Test 4: URL Decoded Path Serving (static files only)
    if test_static_files {
        let res = client
            .get(format!("{}/static/my%20file.css", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert_eq!(text.trim(), "body { background: blue; }");
    }

    // 8. Static Files Content-Types, 404, zero-byte file, path traversal
    if test_static_files {
        // js
        let res = client
            .get(format!("{}/static/app.js", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()
                .get("Content-Type")
                .unwrap()
                .to_str()?
                .contains("javascript")
        );
        assert!(res.text().await?.contains("console.log"));

        // css
        let res = client
            .get(format!("{}/static/app.css", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()
                .get("Content-Type")
                .unwrap()
                .to_str()?
                .contains("css")
        );

        // html
        let res = client
            .get(format!("{}/static/app.html", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()
                .get("Content-Type")
                .unwrap()
                .to_str()?
                .contains("html")
        );

        // wasm
        let res = client
            .get(format!("{}/static/app.wasm", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()
                .get("Content-Type")
                .unwrap()
                .to_str()?
                .contains("wasm")
        );

        // zero-byte
        let res = client
            .get(format!("{}/static/zero.txt", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.bytes().await?.len(), 0);

        // nested icons/logo.svg
        let res = client
            .get(format!("{}/static/assets/icons/logo.svg", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()
                .get("Content-Type")
                .unwrap()
                .to_str()?
                .contains("svg")
        );

        // 404 missing file
        let res = client
            .get(format!("{}/static/does-not-exist.png", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        // path traversal check
        let res = client
            .get(format!("{}/static/../Cargo.toml", base_url))
            .send()
            .await?;
        assert!(
            res.status() == StatusCode::NOT_FOUND
                || res.status() == StatusCode::BAD_REQUEST
        );
    }

    // 9. SSR Modes
    {
        // Async mode
        let res = client.get(format!("{}/ssr/async", base_url)).send().await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(text.contains("Async View"));
        assert!(text.contains("Async resource resolved"));

        // InOrder mode
        let res = client
            .get(format!("{}/ssr/in-order", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(text.contains("InOrder View"));
        assert!(text.contains("InOrder resource resolved"));

        // OutOfOrder mode (with chunked encoding assertion)
        let res = client
            .get(format!("{}/ssr/out-of-order", base_url))
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        if test_static_files {
            // Only assert chunked encoding for Wasmtime; Spin may buffer responses
            let is_chunked = res
                .headers()
                .get("Transfer-Encoding")
                .map(|v| v.to_str().unwrap().contains("chunked"))
                .unwrap_or(false);
            assert!(
                is_chunked,
                "OutOfOrder SSR mode must use chunked transfer encoding"
            );
        }
        let text = res.text().await?;
        assert!(text.contains("OutOfOrder View"));
        assert!(text.contains("OutOfOrder resource resolved"));

        // Meta tags
        let res = client.get(format!("{}/ssr/meta", base_url)).send().await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(text.contains("<title>Meta Test Title</title>"));
        assert!(text.contains(
            r#"<meta name="description" content="Meta Test Description""#
        ));

        // SSR Panic
        let res = client.get(format!("{}/ssr/panic", base_url)).send().await;
        if test_static_files {
            // Wasmtime returns 500 Internal Server Error
            let res = res?;
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        } else {
            // Spin may drop the connection or return a non-200 status
            match res {
                Ok(res) => assert_ne!(
                    res.status(),
                    StatusCode::OK,
                    "SSR panic endpoint should not return 200 OK"
                ),
                Err(_) => { /* connection dropped — acceptable for Spin */ }
            }
        }
    }

    // 10. Request Body size limit check (Wasmtime only — Spin has its own limits)
    if test_static_files {
        // 15MB: should get HTTP 200 OK
        println!("Testing 15MB payload upload...");
        let payload_15mb = vec![b'a'; 15 * 1024 * 1024];
        let payload_str = String::from_utf8(payload_15mb).unwrap();
        let body_json = serde_json::json!({
            "data": payload_str
        });
        let res = client
            .post(format!("{}/api/large_body_test", base_url))
            .header("Content-Type", "application/json")
            .json(&body_json)
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::OK);
        let text = res.text().await?;
        assert!(
            text.contains("15728640"),
            "Expected 15MB string length returned, got: {}",
            text
        );

        // 17MB: should get HTTP 413 Payload Too Large
        println!("Testing 17MB payload upload...");
        let payload_17mb = vec![b'a'; 17 * 1024 * 1024];
        let payload_str_17 = String::from_utf8(payload_17mb).unwrap();
        let body_json_17 = serde_json::json!({
            "data": payload_str_17
        });
        let res = client
            .post(format!("{}/api/large_body_test", base_url))
            .header("Content-Type", "application/json")
            .json(&body_json_17)
            .send()
            .await?;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    println!("All assertions passed successfully!");
    Ok(())
}

async fn run_e2e_tests(wasm_path: &str, is_p3: bool) {
    let server = start_server(wasm_path, is_p3)
        .expect("Failed to start Wasmtime server");
    run_assertions(server.port, true) // true = test static files (Wasmtime serves them)
        .await
        .expect("Assertions failed");
}

#[tokio::test]
#[ignore] // Run via ./run_tests.sh (requires wasmtime + pre-built WASM guests)
async fn test_e2e_wasip2() {
    run_e2e_tests("tests/test-app-p2.wasm", false).await;
}

#[tokio::test]
#[ignore] // Run via ./run_tests.sh (requires wasmtime + pre-built WASM guests)
async fn test_e2e_wasip3() {
    run_e2e_tests("tests/test-app-p3.wasm", true).await;
}

// ---------------------------------------------------------------------------
// Spin server support
// ---------------------------------------------------------------------------

struct SpinServer {
    child: Child,
    port: u16,
    stdout_log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Drop for SpinServer {
    fn drop(&mut self) {
        println!("Stopping Spin server on port {}", self.port);
        let _ = self.child.kill();
        let _ = self.child.wait();

        if std::thread::panicking()
            && let Ok(log) = self.stdout_log.lock()
        {
            eprintln!(
                "\n--- Spin Server Log on port {} (test failed) ---",
                self.port
            );
            for line in log.iter() {
                eprintln!("{}", line);
            }
            eprintln!("--------------------------------------------------");
        }
    }
}

fn start_spin_server(manifest_path: &str) -> anyhow::Result<SpinServer> {
    let args = vec!["up", "-f", manifest_path, "--listen", "127.0.0.1:0"];
    println!("Spawning spin {:?}", args);
    let mut child = Command::new("spin")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to take stdout"))?;
    let mut reader = BufReader::new(stdout);

    let mut port = None;
    let mut line = String::new();
    let stdout_log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stdout_log_clone = stdout_log.clone();

    // Parse "Serving http://127.0.0.1:PORT" from stdout
    for _ in 0..100 {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim().to_string();
        if let Ok(mut log) = stdout_log_clone.lock() {
            log.push(trimmed.clone());
        }
        println!("spin output: {}", trimmed);
        // Look for "Serving http://127.0.0.1:PORT"
        let p = trimmed
            .contains("Serving http")
            .then(|| {
                trimmed
                    .split("http://")
                    .nth(1)
                    .and_then(|addr| addr.trim().trim_end_matches('/').rsplit(':').next())
                    .and_then(|port_str| port_str.parse::<u16>().ok())
            })
            .flatten();
        if let Some(p) = p {
            port = Some(p);
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow::anyhow!(
                "spin up exited early with status: {}",
                status
            ));
        }
    }

    let port = port.ok_or_else(|| {
        anyhow::anyhow!("Failed to parse port from spin up output")
    })?;

    // Drain stdout in background
    std::thread::spawn(move || {
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            let trimmed = line.trim().to_string();
            if let Ok(mut log) = stdout_log_clone.lock() {
                log.push(trimmed);
            }
            line.clear();
        }
    });

    Ok(SpinServer {
        child,
        port,
        stdout_log,
    })
}

async fn run_spin_e2e_tests(manifest_path: &str) {
    let server =
        start_spin_server(manifest_path).expect("Failed to start Spin server");
    run_assertions(server.port, false) // false = skip static file / body limit tests
        .await
        .expect("Assertions failed");
}

#[tokio::test]
#[ignore] // Run via ./run_tests.sh (requires spin + pre-built WASM guests)
async fn test_e2e_spin() {
    run_spin_e2e_tests("tests/spin.toml").await;
}
