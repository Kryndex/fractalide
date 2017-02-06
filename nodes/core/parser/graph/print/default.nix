{ agent, edges, crates, pkgs }:

agent {
  src = ./.;
  edges = with edges; [ CoreGraph ];
  crates = with crates; [ rustfbp capnp ];
  osdeps = with pkgs; [];
}
