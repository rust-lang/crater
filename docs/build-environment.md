# Crater build environment

Crater build all the crates in a sandboxed build environment. This document
contains the details of the production environment, and instructions on how to
tweak it during development.

## Production build environment

All production builds run in the following environment:

* Operative System: **Ubuntu 18.04 LTS**
* RAM: **1.5 GB**
* Network access: **disabled**
* Maximum execution time: **15 minutes**
* Maximum execution time with no output: **2 minutes**

The build images also contain a lot of dependencies pre-installed. If you need
some that aren't available please open an issue on this repository!

## Available environments locally

When working with a local instance of Crater, you can choose the build
environment you want for your builds:

* `mini` (default): small environment with only OpenSSL preinstalled
* `full`: the same environment used in production (it's really heavy though)

To choose the environment you want you need to provide the `--docker-env` flag
to the `run-graph` command:

```
$ cargo run -- run-graph --ex foo --docker-env=full
```

You can also provide a custom Docker image from Docker Hub by passing the full
image name (for example `org/name`) to the `--docker-env` flag. Please note
that Crater expect the image to have some tweaks, so you can't use arbitrary
images.

## Tweaking the build environment locally

If you want to tinker with the build environment you can edit the Dockerfile in
`docker/env/Dockerfile`. That Dockerfile is multi-stage, and you should use the
`build.sh` script to create new builds, passing the environment name as the
first argument:

```
$ docker/env/build.sh mini
```

Then you can suffix the environment name with `@local` to force Crater to use
the local one (instead of fetching it from Docker Hub):

```
$ cargo run -- run-graph --ex foo --docker-env=mini@local
```
