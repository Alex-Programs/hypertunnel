# hypertunnel
Bidirectional SOCKS data smuggling using a HTTP-based middleware. Primarily designed to deal with TLS-decrypting corporate/educational firewalls that other methods are blocked by (due to decryption showing that it actually isn't HTTPS traffic, or ports other than 80/443 being blocked).

This was mostly a research project, and while it works it's quite temperamental. The codebase is also pretty poor.

I'm planning to clean it up before September, as it looks like my uni will have a DPI firewall.
