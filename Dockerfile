# 构建阶段：Node 打前端产物，Rust 打后端二进制
FROM node:24-alpine AS frontend
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml* ./
RUN pnpm install --frozen-lockfile
COPY . .
RUN pnpm build

FROM rust:1.98-slim AS backend
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=frontend /app/dist ./dist
COPY src-tauri ./src-tauri
WORKDIR /app/src-tauri
RUN cargo build --release

# 运行阶段：只保留二进制与 webview 运行时
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=backend /app/src-tauri/target/release/hongguo-downloader /usr/local/bin/
ENTRYPOINT ["hongguo-downloader"]
