output "endpoint" {
  description = "Point S3 clients at this URL"
  value       = "http://${aws_lb.internal.dns_name}:8014"
}

output "task_role_arn" {
  value = aws_iam_role.task.arn
}

output "security_group_id" {
  value = aws_security_group.gateway.id
}
