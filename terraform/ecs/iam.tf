resource "aws_iam_role" "ecs_task_execution_role" {
  name               = "${var.app_name}-ecs-task-execution-role"
  assume_role_policy = data.aws_iam_policy_document.assume_role_policy.json
}

data "aws_iam_policy_document" "assume_role_policy" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }
  }
}

resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

# Prometheus Access
resource "aws_iam_role_policy_attachment" "prometheus_write_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonPrometheusRemoteWriteAccess"
}

# CloudWatch Access
resource "aws_iam_role_policy_attachment" "cloudwatch_write_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/CloudWatchLogsFullAccess"
}

resource "aws_iam_role_policy_attachment" "ssm_read_only_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMReadOnlyAccess"
}

resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_xray_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/AWSXRayDaemonWriteAccess"
}

# OpenTelemetry
data "aws_iam_policy_document" "otel" {
  statement {
    actions = [
      "logs:PutLogEvents",
      "logs:CreateLogGroup",
      "logs:CreateLogStream",
      "logs:DescribeLogStreams",
      "logs:DescribeLogGroups",
      "xray:PutTraceSegments",
      "xray:PutTelemetryRecords",
      "xray:GetSamplingRules",
      "xray:GetSamplingTargets",
      "xray:GetSamplingStatisticSummaries",
      "ssm:GetParameters",
    ]
    resources = [
      "*"
    ]
  }
}

resource "aws_iam_policy" "otel" {
  name   = "${var.app_name}-otel"
  path   = "/"
  policy = data.aws_iam_policy_document.otel.json
}

resource "aws_iam_role_policy_attachment" "ecs_task_execution_fetch_ghcr_secret_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = aws_iam_policy.otel.arn
}

# Analytics Bucket Access
#tfsec:ignore:aws-iam-no-policy-wildcards
resource "aws_iam_policy" "analytics-data-lake_bucket_access" {
  name        = "${var.app_name}_analytics-data-lake_bucket_access"
  path        = "/"
  description = "Allows ${var.app_name} to read/write from ${var.analytics_datalake_bucket_name}"

  policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "ListObjectsInAnalyticsBucket",
        "Effect" : "Allow",
        "Action" : ["s3:ListBucket"],
        "Resource" : ["arn:aws:s3:::${var.analytics_datalake_bucket_name}"]
      },
      {
        "Sid" : "AllObjectActionsInAnalyticsBucket",
        "Effect" : "Allow",
        "Action" : ["s3:*Object"],
        "Resource" : ["arn:aws:s3:::${var.analytics_datalake_bucket_name}/*"]
      },
      {
        "Sid" : "AllGenerateDataKeyForAnalyticsBucket",
        "Effect" : "Allow",
        "Action" : ["kms:GenerateDataKey"],
        "Resource" : [var.analytics_key_arn]
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "analytics-data-lake-bucket-policy-attach" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = aws_iam_policy.analytics-data-lake_bucket_access.arn
}

# GeoIP Bucket Access
resource "aws_iam_policy" "geoip_bucket_access" {
  name        = "${var.app_name}_geoip_bucket_access"
  path        = "/"
  description = "Allows ${var.app_name} to read from ${var.analytics_geoip_db_bucket_name}"

  policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "ListObjectsInGeoipBucket",
        "Effect" : "Allow",
        "Action" : ["s3:ListBucket"],
        "Resource" : ["arn:aws:s3:::${var.analytics_geoip_db_bucket_name}"]
      },
      {
        "Sid" : "AllObjectActionsInGeoipBucket",
        "Effect" : "Allow",
        "Action" : ["s3:CopyObject", "s3:GetObject", "s3:HeadObject"],
        "Resource" : ["arn:aws:s3:::${var.analytics_geoip_db_bucket_name}/*"]
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "geoip-bucket-policy-attach" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = aws_iam_policy.geoip_bucket_access.arn
}