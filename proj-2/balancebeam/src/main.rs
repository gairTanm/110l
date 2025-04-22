mod request;
mod response;

use clap::Parser;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Contains information parsed from the command-line invocation of balancebeam. The Clap macros
/// provide a fancy way to automatically construct a command-line argument parser. #[derive(Parser, Debug)]
#[derive(Parser, Debug)]
#[command(about = "Fun with load balancing")]
struct CmdOptions {
    /// "IP/port to bind to"
    #[arg(short, long, default_value = "0.0.0.0:1100")]
    bind: String,
    /// "Upstream host to forward requests to"
    #[arg(short, long)]
    upstream: Vec<String>,
    /// "Perform active health checks on this interval (in seconds)"
    #[arg(long, default_value = "2")]
    active_health_check_interval: usize,
    /// "Path to send request to for active health checks"
    #[arg(long, default_value = "/")]
    active_health_check_path: String,
    /// "Maximum number of requests to accept per IP per minute (0 = unlimited)"
    #[arg(long, default_value = "0")]
    max_requests_per_minute: usize,
}

/// Contains information about the state of balancebeam (e.g. what servers we are currently proxying
/// to, what servers have failed, rate limiting counts, etc.)
///
/// You should add fields to this struct in later milestones.
#[derive(Clone)]
struct ProxyState {
    /// How frequently we check whether upstream servers are alive (Milestone 4)
    #[allow(dead_code)]
    active_health_check_interval: usize,
    /// Where we should send requests when doing active health checks (Milestone 4)
    #[allow(dead_code)]
    active_health_check_path: String,
    /// Maximum number of requests an individual IP can make in a minute (Milestone 5)
    #[allow(dead_code)]
    max_requests_per_minute: usize,
    /// Addresses of servers that we are proxying to
    upstream_addresses: Arc<Mutex<HashMap<String, bool>>>,
    /// Counter to keep track of the next upstream server to pick
    next_connection: Arc<Mutex<usize>>,

    rate_limiter_service: Arc<Mutex<RateLimiterService>>,
}

impl ProxyState {
    pub async fn get_connection_index(&self, count: usize) -> usize {
        let mut next_connection_idx = self.next_connection.lock().await;
        *next_connection_idx += 1;
        *next_connection_idx %= count;
        *next_connection_idx
    }
}

struct RateLimiterService {
    max_requests_per_minute: usize,

    client_request_count_map: Arc<Mutex<HashMap<String, HashMap<u64, usize>>>>,
}

impl RateLimiterService {
    fn get_current_window(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // window size 60s
        now / std::time::Duration::from_secs(60).as_secs()
    }

    pub async fn should_rate_limit(&mut self, client: &String, port: &String) -> bool {
        if self.max_requests_per_minute == 0 {
            return false;
        };
        let window = self.get_current_window();

        let mut state = self.client_request_count_map.lock().await;
        let key = format!("{}{}", client.clone(), port.clone());
        let bucket_count_for_client = state.entry(key.clone()).or_default();
        let count = bucket_count_for_client.entry(window).or_insert(0);

        log::info!("For {} the count is {} in window {}", key, count, window);
        if *count < self.max_requests_per_minute {
            *count += 1;
            false
        } else {
            true
        }
    }

    pub async fn reset_counts(&mut self) {
        self.client_request_count_map.lock().await.clear();
    }
}

#[tokio::main]
async fn main() {
    // Initialize the logging library. You can print log messages using the `log` macros:
    // https://docs.rs/log/0.4.8/log/ You are welcome to continue using print! statements; this
    // just looks a little prettier.
    if let Err(_) = std::env::var("RUST_LOG") {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::init();

    // Parse the command line arguments passed to this program
    let options = CmdOptions::parse();
    if options.upstream.len() < 1 {
        log::error!("At least one upstream server must be specified using the --upstream option.");
        std::process::exit(1);
    }

    // Start listening for connections
    let listener = match TcpListener::bind(&options.bind).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Could not bind to {}: {}", options.bind, err);
            std::process::exit(1);
        }
    };
    log::info!("Listening for requests on {}", options.bind);

    let upstream_address_map = move || -> HashMap<String, bool> {
        let mut map = HashMap::new();
        for address in options.upstream {
            map.insert(address, true);
        }
        map
    }();

    let upstream_addresses = Arc::new(Mutex::new(upstream_address_map));

    let rate_limiter_service = Arc::new(Mutex::new(RateLimiterService {
        max_requests_per_minute: options.max_requests_per_minute,
        client_request_count_map: Arc::new(Mutex::new(HashMap::new())),
    }));

    // Handle incoming connections
    let state = Arc::new(ProxyState {
        upstream_addresses,
        active_health_check_interval: options.active_health_check_interval,
        active_health_check_path: options.active_health_check_path,
        max_requests_per_minute: options.max_requests_per_minute,
        next_connection: Arc::new(Mutex::new(0)),
        rate_limiter_service,
    });
    //let state_mutex = Arc::new(Mutex::new(state));

    //let mut worker_threads = Vec::new();

    let health_state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(
                health_state_clone.active_health_check_interval as u64,
            ))
            .await;
            perform_health_check(&health_state_clone).await;
        }
    });

    loop {
        if let Ok((stream, _)) = listener.accept().await {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                //state.rate_limiter_service.lock().await.reset_counts().await;
                handle_connection(stream, state).await;
            });
        }
    }
}

async fn perform_health_check(state: &Arc<ProxyState>) {
    let mut upstream_addresses = state.upstream_addresses.lock().await;
    let client = reqwest::Client::new();
    for (upstream, available) in upstream_addresses.iter_mut() {
        let request_path = state.active_health_check_path.clone();
        let response = client
            .get(&format!("http://{}/{}", upstream, request_path))
            .header("Host", upstream)
            .send()
            .await
            .ok();

        if response.is_some() {
            *available = response.unwrap().status().as_u16() == 200;
            log::info!("Upstream {:?} is available: {:?}", *upstream, *available);
        }
    }
}

async fn connect_to_upstream(state: Arc<ProxyState>) -> Result<Vec<TcpStream>, std::io::Error> {
    let mut upstream_addresses = state.upstream_addresses.lock().await;
    //let upstream_idx = rng.gen_range(0..state.upstream_addresses.len());
    //let upstream_ip = &state.upstream_addresses[upstream_idx];
    let mut stream: Option<TcpStream>;
    let mut streams: Vec<TcpStream> = Vec::new();
    for (upstream_ip, available) in upstream_addresses.iter_mut() {
        if *available {
            stream = match TcpStream::connect(upstream_ip).await {
                Ok(tcp_stream) => Some(tcp_stream),
                Err(err) => {
                    log::warn!("Failed to connect to upstream {}: {}", upstream_ip, err);
                    *available = false;
                    None
                }
            };
            if stream.is_some() {
                log::info!("{:?}", stream);
                streams.push(stream.unwrap());
            };
        }
    }

    if streams.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "couldn't connect to any upstream server",
        ));
    }

    Ok(streams)
}

async fn send_response(client_conn: &mut TcpStream, response: &http::Response<Vec<u8>>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!(
        "{} <- {}",
        client_ip,
        response::format_response_line(&response)
    );
    if let Err(error) = response::write_to_stream(&response, client_conn).await {
        log::warn!("Failed to send response to client: {}", error);
        return;
    };
}

async fn handle_connection(mut client_conn: TcpStream, state: Arc<ProxyState>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("Connection received from {}", client_ip);

    // Open a connection to a random destination server
    let mut upstream_conns = match connect_to_upstream(state.clone()).await {
        Ok(stream) => stream,
        Err(_error) => {
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
    };
    let mut upstream_ips = Vec::new();
    for upstream_conn in upstream_conns.iter() {
        upstream_ips.push(upstream_conn.peer_addr().unwrap().ip().to_string());
    }
    //log::info!("Working upstreams: {:?} {:?}", upstream_ips, upstream_conns);
    // The client may now send us one or more requests. Keep trying to read requests until the
    // client hangs up or we get an error.
    loop {
        let state = Arc::clone(&state);
        let idx = state.get_connection_index(upstream_ips.len()).await;
        //log::info!("routing to: {:?}", idx);
        // Read a request from the client
        let mut request = match request::read_from_stream(&mut client_conn).await {
            Ok(request) => request,
            // Handle case where client closed connection and is no longer sending requests
            Err(request::Error::IncompleteRequest(0)) => {
                log::debug!("Client finished sending requests. Shutting down connection");
                return;
            }
            // Handle I/O error in reading from the client
            Err(request::Error::ConnectionError(io_err)) => {
                log::info!("Error reading request from client stream: {}", io_err);
                return;
            }
            Err(error) => {
                log::debug!("Error parsing request: {:?}", error);
                let response = response::make_http_error(match error {
                    request::Error::IncompleteRequest(_)
                    | request::Error::MalformedRequest(_)
                    | request::Error::InvalidContentLength
                    | request::Error::ContentLengthMismatch => http::StatusCode::BAD_REQUEST,
                    request::Error::RequestBodyTooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
                    request::Error::ConnectionError(_) => http::StatusCode::SERVICE_UNAVAILABLE,
                });
                send_response(&mut client_conn, &response).await;
                continue;
            }
        };
        log::info!(
            "{} -> {}: {}",
            client_ip,
            upstream_ips[idx],
            request::format_request_line(&request)
        );

        // Add X-Forwarded-For header so that the upstream server knows the client's IP address.
        // (We're the ones connecting directly to the upstream server, so without this header, the
        // upstream server will only know our IP, not the client's.)
        request::extend_header_value(&mut request, "x-forwarded-for", &client_ip);

        let mut rate_limiter_service = state.rate_limiter_service.lock().await;
        let port = client_conn.local_addr().unwrap().port().to_string();
        if rate_limiter_service
            .should_rate_limit(&client_ip, &port)
            .await
        {
            let response = response::make_http_error(http::StatusCode::TOO_MANY_REQUESTS);
            //log::info!("{:?}", response);
            if let Err(error) = response::write_to_stream(&response, &mut client_conn).await {
                log::warn!("Failed to send response to client: {}", error);
                return;
            };
            continue;
        }
        // Forward the request to the server
        if let Err(error) = request::write_to_stream(&request, &mut upstream_conns[idx]).await {
            log::error!(
                "Failed to send request to upstream {}: {}",
                upstream_ips[idx],
                error
            );
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
        log::debug!("Forwarded request to server");

        // Read the server's response
        let response =
            match response::read_from_stream(&mut upstream_conns[idx], request.method()).await {
                Ok(response) => response,
                Err(error) => {
                    log::error!("Error reading response from server: {:?}", error);
                    let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
                    send_response(&mut client_conn, &response).await;
                    return;
                }
            };
        // Forward the response to the client
        send_response(&mut client_conn, &response).await;
        log::debug!("Forwarded response to client");
    }
}
