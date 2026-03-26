resource "aws_ssm_parameter" "github_app_id" {
  name  = "/${var.app_name}/${var.environment}/GITHUB_APP_ID"
  type  = "String"
  value = "PLACEHOLDER"

  lifecycle {
    ignore_changes = [value]
  }
}

resource "aws_ssm_parameter" "github_app_private_key" {
  name  = "/${var.app_name}/${var.environment}/GITHUB_APP_PRIVATE_KEY"
  type  = "SecureString"
  value = "PLACEHOLDER"

  lifecycle {
    ignore_changes = [value]
  }
}

resource "aws_ssm_parameter" "github_webhook_secret" {
  name  = "/${var.app_name}/${var.environment}/GITHUB_WEBHOOK_SECRET"
  type  = "SecureString"
  value = "PLACEHOLDER"

  lifecycle {
    ignore_changes = [value]
  }
}
