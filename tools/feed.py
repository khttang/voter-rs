# feed.py: the lesson C5 flight (60 cycles), as voter-feed sends it, for testing on the Mac
# (voter-feed is C++ and needs Linux clock_nanosleep). python3 tools/feed.py [port]
import socket, sys, time
port = int(sys.argv[1]) if len(sys.argv) > 1 else 5005
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
for k in range(60):
    t = 1000.0 + 2.0 * k
    a = "-" if k in (40, 41) else "%.3f" % (t + 0.5)
    bv = t - 0.3
    if k == 5: bv = t + 100.0
    if k >= 10: bv = t - 0.3 + (k - 10)
    c = "nan" if k >= 50 else ("0" if k == 25 else "%.3f" % t)
    s.sendto(("%s %.3f %s" % (a, bv, c)).encode(), ("127.0.0.1", port))
    time.sleep(0.01)
