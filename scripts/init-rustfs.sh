#!/bin/bash
# Initialize RustFS bucket for development

set -e

echo "Initializing RustFS for development..."

# Wait for RustFS to be ready
echo "Waiting for RustFS to be ready..."
until curl -sf http://localhost:9000/health > /dev/null 2>&1; do
  sleep 1
done

echo "RustFS is ready!"

# Install mc (MinIO Client) if not already installed
# mc is S3-compatible and works with RustFS
if ! command -v mc &> /dev/null; then
  echo "mc (S3-compatible CLI client) not found. Please install it:"
  echo "  - macOS: brew install minio/stable/mc"
  echo "  - Linux: wget https://dl.min.io/client/mc/release/linux-amd64/mc && chmod +x mc && sudo mv mc /usr/local/bin/"
  exit 1
fi

# Configure mc alias
echo "Configuring mc client..."
mc alias set local http://localhost:9000 minioadmin minioadmin > /dev/null

# Create bucket if it doesn't exist
BUCKET_NAME="voicechat"
if mc ls local/$BUCKET_NAME > /dev/null 2>&1; then
  echo "Bucket '$BUCKET_NAME' already exists"
else
  echo "Creating bucket '$BUCKET_NAME'..."
  mc mb local/$BUCKET_NAME
  echo "Bucket '$BUCKET_NAME' created successfully"
fi

# Set bucket policy to allow uploads (private bucket, access via presigned URLs)
echo "Setting bucket policy..."
mc anonymous set none local/$BUCKET_NAME

echo "RustFS initialization complete!"
echo ""
echo "RustFS is ready for file uploads:"
echo "  - API: http://localhost:9000"
echo "  - Console: http://localhost:9001 (minioadmin / minioadmin)"
echo "  - Bucket: $BUCKET_NAME"
