# System dependencies. This is the Nix equivalent of the two apt layers the
# old Dockerfile had, and every entry is here for a reason confirmed from
# Cargo.lock or package.json rather than added speculatively.
{ pkgs }: {
  deps = [
    # rustup rather than pkgs.rustc/pkgs.cargo, because the channel's Rust is
    # too old and the workspace hides that. backend/Cargo.toml declares
    # edition = "2024", which needs >= 1.85; the `stable-24_05` channel ships
    # Cargo 1.77.1 (24_11 is 1.82, still short). The workspace shell reports
    # 1.88 from Replit's own image, so `rustc --version` there says nothing
    # about what the deploy build will use — the observed failure was
    # `feature edition2024 is required ... not stabilized in this version of
    # Cargo (1.77.1)` after a clean build log.
    #
    # Pinning the toolchain in .replit's build command makes the deploy
    # independent of both the Nix channel and the workspace image. Deliberately
    # NOT a rust-toolchain.toml: that would also retarget local `cargo build`,
    # and this is a Replit packaging concern, not a project-wide one.
    pkgs.rustup

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
