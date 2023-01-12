variable "region" {
  type    = string
  default = "eu-central-1"
}

variable "azs" {
  type    = list(string)
  default = ["eu-central-1a", "eu-central-1b", "eu-central-1c"]
}

variable "public_url" {
  type    = string
  default = "http-starter.walletconnect.com"
}

variable "grafana_endpoint" {
  type = string
}

variable "image_version" {
  type    = string
  default = ""
}
