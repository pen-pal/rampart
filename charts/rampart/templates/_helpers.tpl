{{/*
Expand the name of the chart.
*/}}
{{- define "rampart.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
Truncated at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
*/}}
{{- define "rampart.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "rampart.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "rampart.labels" -}}
helm.sh/chart: {{ include "rampart.chart" . }}
{{ include "rampart.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "rampart.selectorLabels" -}}
app.kubernetes.io/name: {{ include "rampart.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "rampart.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "rampart.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Embedded Postgres resource name
*/}}
{{- define "rampart.postgres.fullname" -}}
{{- printf "%s-postgres" (include "rampart.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Resolve the database URL used by the app.
- If postgres.embedded=true, build it from the embedded Postgres service + generated secret.
- Otherwise use externalDatabase.url.
*/}}
{{- define "rampart.databaseUrl" -}}
{{- if .Values.postgres.embedded -}}
postgres://{{ .Values.postgres.user }}:$(POSTGRES_PASSWORD)@{{ include "rampart.postgres.fullname" . }}:5432/{{ .Values.postgres.database }}
{{- else -}}
{{ required "externalDatabase.url is required when postgres.embedded=false" .Values.externalDatabase.url }}
{{- end -}}
{{- end }}

{{/*
Resolve / persist the session key.
Prefer a user-provided value; otherwise reuse an existing one from the cluster
(via lookup) so upgrades do not invalidate sessions; otherwise generate one.
*/}}
{{- define "rampart.sessionKey" -}}
{{- if .Values.sessionKey -}}
{{ .Values.sessionKey }}
{{- else -}}
{{- $existing := lookup "v1" "Secret" .Release.Namespace (include "rampart.fullname" .) -}}
{{- if and $existing $existing.data (index $existing.data "SESSION_KEY") -}}
{{ index $existing.data "SESSION_KEY" | b64dec }}
{{- else -}}
{{ randAlphaNum 64 }}
{{- end -}}
{{- end -}}
{{- end }}

{{/*
Resolve / persist the embedded-postgres password.
*/}}
{{- define "rampart.postgresPassword" -}}
{{- if .Values.postgres.password -}}
{{ .Values.postgres.password }}
{{- else -}}
{{- $existing := lookup "v1" "Secret" .Release.Namespace (include "rampart.postgres.fullname" .) -}}
{{- if and $existing $existing.data (index $existing.data "POSTGRES_PASSWORD") -}}
{{ index $existing.data "POSTGRES_PASSWORD" | b64dec }}
{{- else -}}
{{ randAlphaNum 32 }}
{{- end -}}
{{- end -}}
{{- end }}
