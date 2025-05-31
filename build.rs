mod build_chromaprint;
mod build_wavpack;

fn main() {
    build_chromaprint::build();
    build_wavpack::build();
}
