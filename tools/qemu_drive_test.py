import socket, time, sys

port = int(sys.argv[1]) if len(sys.argv) > 1 else 4561

mon = socket.create_connection(("127.0.0.1", port), timeout=5)
mon.settimeout(1)

def mon_recv():
    try:
        return mon.recv(65536)
    except socket.timeout:
        return b""

time.sleep(1)
mon_recv()

def sendkey(k):
    mon.sendall(f"sendkey {k}\n".encode())
    time.sleep(0.06)
    mon_recv()

def type_str(s):
    for ch in s:
        if ch == ' ':
            sendkey('spc')
        else:
            sendkey(ch.lower())
    sendkey('ret')

time.sleep(8)
type_str("root")
time.sleep(1)
type_str("root")
time.sleep(3)
type_str("ls")
time.sleep(2)
type_str("echo shell_ok")
time.sleep(2)
type_str("busybox ash -c 'echo forked'")
time.sleep(2)
type_str("pwd")
time.sleep(2)
print("done")
