variable "aws_region" {
  type    = string
  default = "ap-northeast-1"
}

variable "environment" {
  type    = string
  default = "prod"
}

variable "app_name" {
  type    = string
  default = "metsuke"
}

variable "container_port" {
  type    = number
  default = 8080
}

variable "cpu" {
  type    = number
  default = 256
}

variable "memory" {
  type    = number
  default = 512
}

variable "desired_count" {
  type    = number
  default = 1
}

variable "github_repo" {
  type        = string
  default     = "HikaruEgashira/metsuke"
  description = "GitHub repository for OIDC trust (owner/repo)"
}
