FROM ubuntu:16.10

RUN apt-get update
RUN apt-get install gcc -y

ENV PATH "/cargo-home/bin:$PATH"
ENV CARGO_HOME /cargo-home
ENV RUSTUP_HOME /rustup-home
ENV TEST_DIR /test
ENV CARGO_TARGET_DIR /target

WORKDIR /test
