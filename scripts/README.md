# Development Scripts

This directory contains utility scripts for development and maintenance.

## File Upload Setup

### `init-rustfs.sh`

Initializes RustFS (S3-compatible storage) for local development.

**Prerequisites**: `mc` ([MinIO Client](https://min.io/docs/minio/linux/reference/minio-mc.html)) installed locally, or use Docker method (recommended).

**Usage**:

```bash
# Method 1: Via Docker (recommended - no local installation needed)
docker run --rm --network container:canis-dev-rustfs --entrypoint sh minio/mc -c "\
  mc alias set local http://localhost:9000 rustfsdev rustfsdev_secret && \
  mc mb --ignore-existing local/voicechat && \
  mc anonymous set none local/voicechat"

# Method 2: Using the script (requires mc client on host)
./scripts/init-rustfs.sh
```

**What it does**:
1. Configures mc client to connect to local RustFS instance
2. Creates the `voicechat` bucket if it doesn't exist
3. Sets bucket policy to private (access via presigned URLs only)

**When to run**:
- First time setting up file uploads
- After running `docker compose down -v` (removes volumes)
- If file uploads return "Bucket not found" errors

See [File Uploads Development Guide](../docs/development/file-uploads.md) for complete setup instructions.
