FROM rust:1.41-alpine
WORKDIR /code
COPY . .
RUN cargo build
