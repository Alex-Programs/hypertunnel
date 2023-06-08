libsocks contains all socks utility code on the client and server
libtransit contains all core tranit code on the client and server

client-core links libsocks to the socks client conns to libtransit and manages buffer control between them. Exposes events and settings.
client-cli wraps around client-core as a command-line-interface for dev
client-gui does the same with a basic gui

server-core links libsocks to the socks server conns to libtransit and manages buffer control between them. Takes settings, outputs logs.
server-cli is just the wrapper around that

