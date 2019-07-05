{
  name = "jormungandr";

  epoch = 201906;

  description = "A Cardano node written in Rust";

  inputs =
    [ "nixpkgs"
      github:edolstra/import-cargo
    ];

  outputs = inputs:
    with import inputs.nixpkgs { system = "x86_64-linux"; };
    with pkgs;

    rec {

      defaultPackage = stdenv.mkDerivation {
        name = "jormungandr-${lib.substring 0 8 inputs.self.lastModified}-${inputs.self.shortRev or "0000000"}";

        buildInputs =
          [ rustc
            cargo
            sqlite
            protobuf
            rustfmt
            pkgconfig
            openssl
            (inputs.import-cargo.builders.importCargo {
              lockFile = ./Cargo.lock;
              inherit pkgs;
            }).cargoHome
          ];

        # FIXME: we can remove this once prost is updated.
        PROTOC = "${protobuf}/bin/protoc";

        src = inputs.self;

        postUnpack =
          ''
            # FIXME: add submodule support to Nix
            ln -s ${builtins.fetchTarball {
              url = "https://github.com/input-output-hk/chain-libs/archive/9c1111a2e842439653be04351e983bf63105ba36.tar.gz";
              sha256 = "15bma8cldkfqd5183m7akzgrp36ayhxq4f2sscrl86mbk3qdawwm";
            }} $sourceRoot/chain-deps
          '';

        buildPhase = ":";

        installPhase =
          ''
            mkdir -p $out
            cargo install -Z offline --root $out --path jormungandr
            cargo install -Z offline --root $out --path jcli
            rm $out/.crates.toml
          '';
      };

      checks.build = defaultPackage;

    };
}
