rm -rf backend/static
cd frontend || exit
trunk build --release
cd ../backend || exit
cargo clean
cargo run