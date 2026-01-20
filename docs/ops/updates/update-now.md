# 🚨 IMMEDIATE ACTIONS REQUIRED

## Critical Issue: Outdated Rust Toolchain

Your system has **Rust 1.75.0** (December 2023), but the project requires **Rust 1.82+**.
This is preventing tests from running and blocking development.

---

## 🔧 Fix It Now (5 minutes)

### Step 1: Update Rust
```bash
# Update rustup itself
rustup self update

# Update to latest stable Rust
rustup update stable

# Set as default
rustup default stable

# Verify (should show 1.84+)
rustc --version
cargo --version
```

### Step 2: Rebuild Project
```bash
cd /home/detair/GIT/canis

# Clean old builds
cargo clean

# Rebuild with new Rust version
cargo build --release

# Run tests (should now work!)
cargo test --workspace
```

### Step 3: Verify Voice Features Work
```bash
# Start server
cargo run --release

# In another terminal, run client
cd client
npm run dev
```

---

## 📦 Safe npm Updates (Do After Rust)

These are **non-breaking** patch/minor updates you can apply safely:

```bash
cd /home/detair/GIT/canis/client

# Update safe packages
npm install @tauri-apps/plugin-shell@^2.3.4
npm install lucide-solid@^0.562.0
npm install eslint-plugin-solid@^0.14.5

# Rebuild to verify
npm run build
npm run tauri build
```

---

## ⏳ Schedule for Later (Breaking Changes)

These require code changes and testing:

### Week 2: Backend Updates
- axum 0.7 → 0.8 (2-4 hours)
- sqlx 0.7 → 0.8 (1-2 hours)

### Week 3: Frontend Updates
- vite 5 → 7 (1-2 hours)
- eslint 8 → 9 (1 hour)
- @solidjs/router 0.10 → 0.15 (2-3 hours)

### Research Needed
- **UnoCSS** 0.58 → 66.5 (huge version jump - investigate first)

---

## ✅ Expected Results After Rust Update

After updating Rust, you should see:
- ✅ Tests run successfully
- ✅ No more `edition2024` errors
- ✅ Cargo build completes without warnings about future incompatibilities
- ✅ Access to 12+ months of Rust performance improvements
- ✅ Latest security patches

---

## 🆘 If Something Breaks

### Rust Update Issues
```bash
# Rollback to previous Rust (if needed)
rustup default 1.75.0

# Or use specific version
rustup install 1.82.0
rustup default 1.82.0
```

### Cargo Build Issues
```bash
# Clear all caches
cargo clean
rm -rf ~/.cargo/registry/cache
rm -rf ~/.cargo/git/db

# Try again
cargo build
```

### npm Issues
```bash
# Nuclear option: rebuild node_modules
cd client
rm -rf node_modules package-lock.json
npm install
```

---

## 📝 Checklist

- [ ] Update Rust toolchain to 1.84+
- [ ] Verify `cargo --version` shows 1.84+
- [ ] Run `cargo clean`
- [ ] Run `cargo build --release` successfully
- [ ] Run `cargo test --workspace` - all tests pass
- [ ] Start server, verify no errors
- [ ] Test voice chat functionality
- [ ] Update safe npm packages
- [ ] Commit changes
- [ ] Schedule breaking updates for next sprint

---

**Priority**: 🔴 DO THIS NOW before any other development work

**Time Required**: 5-10 minutes

**Risk**: Very Low (Rust updates are designed to be backward compatible)
