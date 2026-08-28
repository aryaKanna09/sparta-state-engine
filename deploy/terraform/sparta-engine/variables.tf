variable "name" {
  description = "Resource name prefix"
  type        = string
  default     = "sparta-engine"
}

variable "region" {
  type = string
}

variable "vpc_id" {
  description = "Existing VPC to deploy into"
  type        = string
}

variable "subnet_ids" {
  description = "Private subnets for the service and NLB"
  type        = list(string)
}

variable "bucket_name" {
  description = "The customer bucket the engine serves and stores _sparta/ in"
  type        = string
}

variable "upstream_endpoint" {
  description = "S3 endpoint URL (regional endpoint or interface VPC endpoint)"
  type        = string
}

variable "image" {
  description = "Sparta engine container image (pushed to your registry)"
  type        = string
}

variable "mode" {
  type    = string
  default = "shadow"
}

variable "bypass" {
  description = "Start in dumb-proxy bypass mode"
  type        = bool
  default     = false
}

variable "gc_grace_days" {
  type    = number
  default = 7
}

variable "cpu" {
  type    = number
  default = 2048
}

variable "memory" {
  type    = number
  default = 8192
}

variable "cache_gib" {
  description = "Fargate ephemeral storage for the chunk cache"
  type        = number
  default     = 50
}

variable "allowed_cidrs" {
  description = "CIDRs allowed to reach the gateway"
  type        = list(string)
  default     = ["10.0.0.0/8"]
}

variable "cluster_arn" {
  description = "Existing ECS cluster ARN (empty = create one)"
  type        = string
  default     = ""
}

variable "credentials_secret_arn" {
  description = "Secrets Manager secret with accessKey/secretKey for client-side auth (empty = anonymous dev mode)"
  type        = string
  default     = ""
}

variable "extra_env" {
  description = "Additional SPARTA_* environment variables"
  type        = map(string)
  default     = {}
}
