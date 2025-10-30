rm -rf backend/static
cd frontend || exit
trunk build --release
cd ../ || exit
cargo clean
cargo build -p backend --release