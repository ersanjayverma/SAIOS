#!/usr/bin/env python3
"""
SAIOS Supplier - Local file server for SAIOS development
Serves files from the saios directory for easy downloading in SAIOS VM
"""

import http.server
import socketserver
import os
import sys
from urllib.parse import unquote

PORT = 8080
DIRECTORY = os.path.dirname(os.path.abspath(__file__))

class SAIOSHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIRECTORY, **kwargs)
    
    def log_message(self, format, *args):
        # Custom logging with SAIOS branding
        print(f"[saiossupplier] {args[0]} - {args[1]} {args[2]}")
    
    def end_headers(self):
        # Add CORS headers for easier access
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()
    
    def do_GET(self):
        path = unquote(self.path)
        print(f"[saiossupplier] Serving: {path}")
        super().do_GET()

def main():
    print("=" * 60)
    print("  SAIOS Supplier - Local File Server")
    print("=" * 60)
    print(f"  Directory: {DIRECTORY}")
    print(f"  Port: {PORT}")
    print()
    print("  URLs available:")
    print(f"    http://localhost:{PORT}/")
    print(f"    http://localhost:{PORT}/target/x86_64-unknown-none/debug/saios")
    print(f"    http://localhost:{PORT}/saios.iso")
    print()
    print("  In SAIOS, download files with:")
    print(f"    curl http://10.0.2.2:{PORT}/filename -o /path/to/file")
    print("    (10.0.2.2 is the host IP in QEMU/VirtualBox NAT)")
    print("=" * 60)
    print()
    
    with socketserver.TCPServer(("", PORT), SAIOSHandler) as httpd:
        print(f"[saiossupplier] Server started on port {PORT}")
        print("[saiossupplier] Press Ctrl+C to stop")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\n[saiossupplier] Server stopped")
            sys.exit(0)

if __name__ == "__main__":
    main()
