output "ecr_repository_url" {
  value = aws_ecr_repository.app.repository_url
}

output "alb_dns_name" {
  value = aws_lb.main.dns_name
}

output "mcp_endpoint" {
  value = "http://${aws_lb.main.dns_name}/mcp"
}

output "webhook_endpoint" {
  value = "http://${aws_lb.main.dns_name}/webhook"
}
