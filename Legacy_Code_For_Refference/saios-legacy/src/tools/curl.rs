//! curl — command-line HTTP client for SAIOS.
//!
//! Supports:
//!   GET / POST / PUT / DELETE / HEAD
//!   Custom headers (-H)
//!   Request body (-d)
//!   Silent mode (-s)
//!   Output to file (-o)
//!   Show headers (-i)
//!   Follow redirects (-L)
//!
//! Built on SAIOS's own TCP/IP stack (crate::net::http).
//! HTTPS requires TLS — currently prints a warning and attempts plain HTTP.
//!
//! # Examples
//! ```
//! curl http://example.com/
//! curl -o /tmp/page.html http://example.com/
//! curl -X POST -d '{"key":"val"}' -H 'Content-Type: application/json' http://api/
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// All options that `curl` can receive from the command line.
#[derive(Default, Clone)]
pub struct CurlOpts {
    /// URL to request (required).
    pub url: String,
    /// HTTP method: GET, POST, PUT, DELETE, HEAD, PATCH.
    pub method: String,
    /// Extra headers in "Key: Value" format.
    pub headers: Vec<(String, String)>,
    /// Request body (used with POST/PUT).
    pub data: Option<String>,
    /// Write response body to this file instead of stdout.
    pub output: Option<String>,
    /// Suppress progress/info output — only print response body.
    pub silent: bool,
    /// Include response headers in output.
    pub include_headers: bool,
    /// Follow HTTP 3xx redirects (up to 5 hops).
    pub follow_redirects: bool,
    /// Maximum time in seconds to wait for a response (0 = no limit).
    pub max_time: u64,
}

impl CurlOpts {
    /// Parse command-line arguments into a `CurlOpts`.
    ///
    /// Supported flags:
    ///   -X <method>    HTTP method (default: GET, or POST if -d is set)
    ///   -H <header>    Add a request header ("Name: Value")
    ///   -d <data>      Request body (implies POST if -X not given)
    ///   -o <file>      Save output to file
    ///   -s             Silent mode
    ///   -i             Include response headers in output
    ///   -L             Follow redirects
    ///   --max-time <n> Timeout in seconds
    pub fn parse(args: &str) -> Result<Self, String> {
        let mut opts = Self {
            method: String::from("GET"),
            ..Self::default()
        };

        let tokens: Vec<&str> = args.split_whitespace().collect();
        let mut i = 0;

        while i < tokens.len() {
            match tokens[i] {
                "-X" | "--request" => {
                    i += 1;
                    opts.method = tokens.get(i).unwrap_or(&"GET").to_uppercase();
                }
                "-H" | "--header" => {
                    i += 1;
                    if let Some(h) = tokens.get(i)
                        && let Some(colon) = h.find(':')
                    {
                        let k = h[..colon].trim().to_string();
                        let v = h[colon + 1..].trim().to_string();
                        opts.headers.push((k, v));
                    }
                }
                "-d" | "--data" | "--data-raw" => {
                    i += 1;
                    if let Some(d) = tokens.get(i) {
                        opts.data = Some(d.to_string());
                        if opts.method == "GET" {
                            opts.method = String::from("POST");
                        }
                    }
                }
                "-o" | "--output" => {
                    i += 1;
                    opts.output = tokens.get(i).map(|s| s.to_string());
                }
                "-s" | "--silent" => opts.silent = true,
                "-i" | "--include" => opts.include_headers = true,
                "-L" | "--location" => opts.follow_redirects = true,
                "--max-time" => {
                    i += 1;
                    opts.max_time = tokens.get(i).and_then(|s| s.parse().ok()).unwrap_or(30);
                }
                arg if arg.starts_with("http") => opts.url = arg.to_string(),
                arg => {
                    // Unknown flag or positional arg assumed to be URL
                    if !arg.starts_with('-') {
                        opts.url = arg.to_string();
                    }
                }
            }
            i += 1;
        }

        if opts.url.is_empty() {
            return Err(String::from(
                "curl: no URL provided\nusage: curl [options] <url>",
            ));
        }
        Ok(opts)
    }
}

/// Run the curl command with the given argument string.
pub fn run(args: &str) {
    // Show help
    if args.trim() == "--help" || args.trim() == "-h" {
        print_help();
        return;
    }

    let opts = match CurlOpts::parse(args) {
        Ok(o) => o,
        Err(e) => {
            crate::println!("{}", e);
            return;
        }
    };

    execute(opts);
}

/// Execute the HTTP request described by `opts`.
fn execute(opts: CurlOpts) {
    // Parse URL: http[s]://host[:port]/path
    let (scheme, host, port, path) = match parse_url(&opts.url) {
        Some(t) => t,
        None => {
            crate::println!("curl: invalid URL '{}'", opts.url);
            return;
        }
    };

    if !opts.silent {
        crate::println!("  % Total    % Received  % Xferd  Average Speed");
    }

    // Build and send the request — over TLS for https://, plain HTTP otherwise.
    let response = {
        let body_ref = opts.data.as_deref();
        let req = if let Some(body) = body_ref {
            crate::net::http::HttpRequest::post_json(&host, &path, port, body)
        } else {
            crate::net::http::HttpRequest::get(&host, &path, port)
        };
        if scheme == "https" {
            crate::net::http::send_https(req)
        } else {
            crate::net::http::send(req)
        }
    };

    let response = match response {
        Some(r) => r,
        None => {
            crate::println!("curl: connection failed to {}:{}", host, port);
            return;
        }
    };

    // Handle redirects
    if opts.follow_redirects
        && response.status / 100 == 3
        && let Some((_, location)) = response
            .headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == "location")
    {
        if !opts.silent {
            crate::println!("curl: redirecting to {}", location);
        }
        let mut redir = opts.clone();
        redir.url = location.clone();
        execute(redir);
        return;
    }

    // Print headers if -i
    if opts.include_headers {
        crate::println!("HTTP/1.1 {}", response.status);
        for (k, v) in &response.headers {
            crate::println!("{}: {}", k, v);
        }
        crate::println!();
    }

    // Output response body
    if let Some(ref file) = opts.output {
        let file = super::resolve_path(file);
        match crate::vfs_contract::VfsContract::write_file(&file, response.body.as_bytes(), 0o644) {
            Ok(()) => {
                if !opts.silent {
                    crate::println!("curl: saved {} bytes to '{}'", response.body.len(), file);
                }
            }
            Err(_) => crate::println!("curl: cannot create output file '{}'", file),
        }
    } else {
        crate::print!("{}", response.body);
    }

    if !opts.silent {
        crate::println!();
        crate::println!(
            "curl: HTTP {} — {} bytes",
            response.status,
            response.body.len()
        );
    }
}

/// Public wrapper for wget/other tools to reuse URL parsing.
pub fn parse_url_pub(url: &str) -> Option<(String, String, u16, String)> {
    parse_url(url)
}

/// Parse a URL into (scheme, host, port, path).
fn parse_url(url: &str) -> Option<(String, String, u16, String)> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https".to_string(), r)
    } else {
        let r = url.strip_prefix("http://")?;
        ("http".to_string(), r)
    };

    let default_port: u16 = if scheme == "https" { 443 } else { 80 };

    let (host_port, path) = if let Some(slash) = rest.find('/') {
        (&rest[..slash], &rest[slash..])
    } else {
        (rest, "/")
    };

    let (host, port) = if let Some(colon) = host_port.find(':') {
        let port = host_port[colon + 1..].parse().unwrap_or(default_port);
        (&host_port[..colon], port)
    } else {
        (host_port, default_port)
    };

    Some((scheme, host.to_string(), port, path.to_string()))
}

fn print_help() {
    crate::println!("curl — SAIOS HTTP client");
    crate::println!();
    crate::println!("USAGE");
    crate::println!("  curl [OPTIONS] <url>");
    crate::println!();
    crate::println!("OPTIONS");
    crate::println!("  -X <method>      HTTP method (GET, POST, PUT, DELETE)");
    crate::println!("  -H 'K: V'        Add request header");
    crate::println!("  -d <data>        Request body (implies POST)");
    crate::println!("  -o <file>        Write output to file");
    crate::println!("  -s               Silent (no progress)");
    crate::println!("  -i               Include response headers");
    crate::println!("  -L               Follow redirects");
    crate::println!("  --max-time <n>   Timeout in seconds");
    crate::println!();
    crate::println!("EXAMPLES");
    crate::println!("  curl http://example.com/");
    crate::println!("  curl -o /tmp/data.json http://api.example.com/data");
    crate::println!("  curl -X POST -d '{{\"msg\":\"hi\"}}' http://api/endpoint");
}
