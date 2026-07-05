# ── build ────────────────────────────────────────────────────────────────────
# Compile the Vite SPA to static assets. devDependencies are needed (tsc, vite),
# so do NOT set NODE_ENV=production here.
FROM node:22-alpine AS build
WORKDIR /app
# Cache deps on the lockfile.
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

# ── runtime ──────────────────────────────────────────────────────────────────
# Serve the built SPA and reverse-proxy the API to ingest-api on the internal
# frdcmp network. No Node in the final image.
FROM nginx:1.27-alpine
RUN rm /etc/nginx/conf.d/default.conf
COPY nginx.conf /etc/nginx/conf.d/dashboard.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
