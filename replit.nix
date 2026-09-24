# System dependencies. This is the Nix equivalent of the two apt layers the
# old Dockerfile had, and every entry is here for a reason confirmed from
# Cargo.lock or package.json rather than added speculatively.
{ pkgs }: {
  deps = [
    # backend/Cargo.toml declares edition = "2024" -> Rust >= 1.85 required.
    # If this channel's rustc is older, drop these two, add pkgs.rustup, and
    # install a pinned toolchain from the build command instead.
    pkgs.rustc
    pkgs.cargo

    # Vite 5 needs Node 18+.
    pkgs.nodejs_22

    # reqwest 0.12 with default features resolves to native-tls -> openssl-sys,
    # which needs headers at build time and the library at run time. Switching
    # reqwest to rustls-tls would remove both, but that changes the TLS stack
    # under every fetcher, so it is a deliberate follow-up rather than part of
    # the port.
    pkgs.pkg-config
    pkgs.openssl

    # rusqlite is used with the `bundled` feature, so SQLite is compiled from
    # C source and needs a C toolchain. (flate2 is deliberately left on its
    # default pure-Rust miniz_oxide backend, so no zlib is needed here.)
    pkgs.stdenv.cc

    # Every fetcher is HTTPS — OONI, IODA, Tor Metrics, Cloudflare, RIPEstat,
    # Pulse, CelesTrak, SatNOGS — and so is the App Storage snapshot. Without
    # a trust store all of them fail while the process looks healthy. See
    # SSL_CERT_FILE in .replit.
    pkgs.cacert
  ];
}
