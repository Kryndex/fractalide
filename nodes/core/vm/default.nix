{ agent, edges, crates, pkgs }:

agent {
  src = ./.;
  edges = with edges; [ CoreGraph FsPath FsPathOption ];
  crates = with crates; [ rustfbp capnp ];
  osdeps = with pkgs; [];
}
