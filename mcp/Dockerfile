# clicklog-mcp — remote MCP endpoint (streamable HTTP) over the read API.
FROM node:22-alpine

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm ci --omit=dev

COPY index.js ./

ENV MCP_PORT=3000
EXPOSE 3000

# busybox wget ships with alpine — used by the compose healthcheck too.
HEALTHCHECK --interval=15s --timeout=5s --retries=5 \
  CMD wget -qO- http://127.0.0.1:3000/healthz || exit 1

CMD ["node", "index.js", "--http"]
