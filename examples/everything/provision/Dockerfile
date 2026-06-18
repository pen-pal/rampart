FROM alpine:3.20
RUN apk add --no-cache bash curl jq coreutils
WORKDIR /provision
COPY provision.sh ./
COPY sections/ ./sections/
RUN chmod +x provision.sh
ENTRYPOINT ["bash", "/provision/provision.sh"]
