use crate::net;
use crate::{print, println};

pub fn net(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let sub = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match sub {
        "status" | "diag" => {
            let status = crate::network_contract::NetworkContract::status_view();
            let ip = status.identity.ip;
            let mac = status.identity.mac;
            let dns = status.identity.dns;
            let gateway = status.identity.gateway;
            let driver = status.driver;
            println!("Driver  {}", driver);
            println!("IP      {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
            println!(
                "MAC     {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
            println!("DNS     {}.{}.{}.{}", dns[0], dns[1], dns[2], dns[3]);
            println!(
                "GW      {}.{}.{}.{}  (QEMU/VirtualBox user-mode NAT)",
                gateway[0], gateway[1], gateway[2], gateway[3]
            );
            println!(
                "Queues  tx={} rx={} arp={}",
                status.tx_depth, status.rx_depth, status.arp_entries
            );
            println!(
                "Events  sockets={} tcp={} waits={}",
                status.socket_events, status.tcp_transitions, status.wait_progress
            );
            println!();
            println!("Test:   net ping 10.0.2.2    (ARP probe to gateway)");
            println!("        net dns google.com    (DNS lookup)");
            println!("        fetch http://example.com/");
        }
        "dns" if !rest.is_empty() => {
            print!("Resolving {}... ", rest);
            match net::dns::resolve_blocking(rest) {
                Some(ip) => println!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
                None => println!("failed"),
            }
        }
        "ping" => {
            if let Some(ip) = super::parse_ipv4(rest) {
                net::arp::send_request(ip);
                println!("ARP request sent to {}", rest);
            } else {
                println!("usage: net ping <ip>");
            }
        }
        _ => println!("usage: net status | dns <host> | ping <ip>"),
    }
}

pub fn fetch(args: &str) {
    let url = args.trim().trim_start_matches("http://");
    let (host, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };
    if host.is_empty() {
        println!("usage: fetch http://<host>[/path]");
        return;
    }
    println!("GET http://{}{}", host, path);
    let req = net::http::HttpRequest::get(host, path, 80);
    match net::http::send(req) {
        Some(resp) => {
            println!("HTTP {}", resp.status);
            println!("---------------------");
            for (i, line) in resp.body.lines().enumerate() {
                if i >= 40 {
                    println!("... (truncated)");
                    break;
                }
                println!("{}", line);
            }
        }
        None => println!("fetch: connection failed"),
    }
}

pub fn help_network() {
    println!("  Network:");
    println!("    net status         IP/MAC/DNS");
    println!("    net dns <host>     resolve hostname");
    println!("    net ping <ip>      ARP probe");
    println!("    fetch <url>        HTTP GET (http://host/path)");
}
