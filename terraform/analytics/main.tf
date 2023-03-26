resource "aws_s3_bucket" "analytics-data-lake_bucket" {
  bucket = "walletconnect.${var.app_name}.${var.environment}.analytics.data-lake"
}

resource "aws_s3_bucket_acl" "analytics-data-lake_acl" {
  bucket = aws_s3_bucket.analytics-data-lake_bucket.id
  acl    = "private"
}

resource "aws_s3_bucket_public_access_block" "analytics-data-lake_bucket" {
  bucket = aws_s3_bucket.analytics-data-lake_bucket.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "analytics-data-lake_bucket" {
  bucket = aws_s3_bucket.analytics-data-lake_bucket.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.analytics_bucket.arn
      sse_algorithm     = "aws:kms"
    }
    bucket_key_enabled = true
  }
}

resource "aws_s3_bucket_versioning" "analytics-data-lake_bucket" {
  bucket = aws_s3_bucket.analytics-data-lake_bucket.id

  versioning_configuration {
    status = "Enabled"
  }
}
