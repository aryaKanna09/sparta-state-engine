{{- define "sparta-engine.fullname" -}}
{{- printf "%s" .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "sparta-engine.labels" -}}
app.kubernetes.io/name: sparta-engine
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion }}
{{- end -}}

{{- define "sparta-engine.selectorLabels" -}}
app.kubernetes.io/name: sparta-engine
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
