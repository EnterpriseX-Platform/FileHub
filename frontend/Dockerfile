# FileHub frontend — Next.js 15 (App Router, basePath /filehub).
#
# Uses Next's standalone output: the runtime stage carries only server.js +
# the pruned server bundle + static assets (no node_modules tree).
#
# IMPORTANT: next.config.ts is serialized into the bundle at BUILD time, so
# the /filehub/api/* → backend rewrite target must be known here. Pass the
# in-cluster service URL as a build arg (default matches the K8s Service in
# infra/sit-bb/k8s/):
#
#   docker build --platform=linux/amd64 \
#     --build-arg BACKEND_URL=http://neb-filehub-api:8090 \
#     -t avalantglobal/filehub-web:VERSION -f frontend/Dockerfile frontend/

FROM node:20-alpine AS builder
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
ARG BACKEND_URL=http://neb-filehub-api:8090
ENV BACKEND_URL=${BACKEND_URL}
RUN npm run build

FROM node:20-alpine AS runtime
ENV TZ=Asia/Bangkok NODE_ENV=production HOSTNAME=0.0.0.0 PORT=3000
WORKDIR /app
COPY --from=builder --chown=node:node /app/.next/standalone ./
COPY --from=builder --chown=node:node /app/.next/static ./.next/static
COPY --from=builder --chown=node:node /app/public ./public
USER node
EXPOSE 3000
CMD ["node", "server.js"]
