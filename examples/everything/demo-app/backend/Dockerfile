FROM node:22-alpine
WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev
COPY otel.js server.js ./
ENV NODE_ENV=production
# The shared volume holds env written by `provision` (SENTRY_DSN, endpoints).
# The entrypoint sources it (if present) before starting the app.
COPY entrypoint.sh ./
RUN chmod +x entrypoint.sh
CMD ["./entrypoint.sh"]
