# Sparta State Engine — AWS deployment into an EXISTING VPC.
#
# Runs the gateway as an ECS Fargate service behind an internal NLB, with an
# IAM task role scoped to the customer's bucket. Nothing in this module sends
# data outside the VPC: the gateway talks only to the S3 endpoint you give it.
#
# Minimum inputs: vpc_id, subnet_ids, bucket_name, image.

terraform {
  required_version = ">= 1.5"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

locals {
  name = var.name
  env = merge({
    SPARTA_LISTEN             = "0.0.0.0:8014"
    SPARTA_UPSTREAM_ENDPOINT  = var.upstream_endpoint
    SPARTA_UPSTREAM_REGION    = var.region
    SPARTA_MODE               = var.mode
    SPARTA_GC_GRACE_DAYS      = tostring(var.gc_grace_days)
    SPARTA_BYPASS             = var.bypass ? "1" : "0"
    RUST_LOG                  = "info"
  }, var.extra_env)
}

resource "aws_security_group" "gateway" {
  name_prefix = "${local.name}-"
  vpc_id      = var.vpc_id

  ingress {
    description = "S3 API from inside the VPC"
    from_port   = 8014
    to_port     = 8014
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }
  egress {
    description = "upstream S3 + image pulls"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_iam_role" "task" {
  name_prefix        = "${local.name}-task-"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ecs-tasks.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "bucket_access" {
  name_prefix = "${local.name}-s3-"
  role        = aws_iam_role.task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = ["s3:ListBucket", "s3:GetBucketLocation"]
        Resource = "arn:aws:s3:::${var.bucket_name}"
      },
      {
        Effect = "Allow"
        Action = [
          "s3:GetObject", "s3:PutObject", "s3:DeleteObject",
          "s3:AbortMultipartUpload", "s3:ListMultipartUploadParts",
          "s3:GetObjectTagging", "s3:PutObjectTagging"
        ]
        Resource = "arn:aws:s3:::${var.bucket_name}/*"
      }
    ]
  })
}

resource "aws_iam_role" "execution" {
  name_prefix        = "${local.name}-exec-"
  assume_role_policy = aws_iam_role.task.assume_role_policy
}

resource "aws_iam_role_policy_attachment" "execution" {
  role       = aws_iam_role.execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_ecs_cluster" "this" {
  count = var.cluster_arn == "" ? 1 : 0
  name  = local.name
}

resource "aws_cloudwatch_log_group" "gateway" {
  name              = "/sparta/${local.name}"
  retention_in_days = 30
}

resource "aws_ecs_task_definition" "gateway" {
  family                   = local.name
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = var.cpu
  memory                   = var.memory
  task_role_arn            = aws_iam_role.task.arn
  execution_role_arn       = aws_iam_role.execution.arn

  ephemeral_storage {
    size_in_gib = var.cache_gib
  }

  container_definitions = jsonencode([{
    name      = "gateway"
    image     = var.image
    command   = ["serve"]
    essential = true
    portMappings = [{ containerPort = 8014, protocol = "tcp" }]
    environment = [for k, v in local.env : { name = k, value = v }]
    # Credentials come from the task role via the SDK default chain when
    # SPARTA_ACCESS_KEY is unset upstream-side; client-side auth keys should
    # come from Secrets Manager:
    secrets = var.credentials_secret_arn == "" ? [] : [
      { name = "SPARTA_ACCESS_KEY", valueFrom = "${var.credentials_secret_arn}:accessKey::" },
      { name = "SPARTA_SECRET_KEY", valueFrom = "${var.credentials_secret_arn}:secretKey::" }
    ]
    healthCheck = {
      command  = ["CMD-SHELL", "sparta-gateway stats >/dev/null 2>&1 || exit 0"]
      interval = 10
      retries  = 3
    }
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.gateway.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "gateway"
      }
    }
  }])
}

resource "aws_lb" "internal" {
  name_prefix        = "spart"
  internal           = true
  load_balancer_type = "network"
  subnets            = var.subnet_ids
}

resource "aws_lb_target_group" "gateway" {
  name_prefix = "spart"
  port        = 8014
  protocol    = "TCP"
  target_type = "ip"
  vpc_id      = var.vpc_id
  health_check {
    protocol = "HTTP"
    path     = "/readyz"
    port     = "8014"
    interval = 10
    healthy_threshold   = 2
    unhealthy_threshold = 2
  }
}

resource "aws_lb_listener" "gateway" {
  load_balancer_arn = aws_lb.internal.arn
  port              = 8014
  protocol          = "TCP"
  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.gateway.arn
  }
}

resource "aws_ecs_service" "gateway" {
  name            = local.name
  cluster         = var.cluster_arn != "" ? var.cluster_arn : aws_ecs_cluster.this[0].arn
  task_definition = aws_ecs_task_definition.gateway.arn
  desired_count   = 1   # single-node metadata by design
  launch_type     = "FARGATE"

  network_configuration {
    subnets          = var.subnet_ids
    security_groups  = [aws_security_group.gateway.id]
    assign_public_ip = false
  }
  load_balancer {
    target_group_arn = aws_lb_target_group.gateway.arn
    container_name   = "gateway"
    container_port   = 8014
  }
  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100
}
