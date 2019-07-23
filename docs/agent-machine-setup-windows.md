# Setting up a new Crater Windows agent machine

## Operating System

The agent machine should run Windows Server build 1803 (Windows 10 build 1803
will **not** work, see below). The machine should have Hyper-V enabled (images
ending in `v3` on Azure DevOps), although this may not be necessary since we
only use process isolation. Testing has been done on a server with a GUI, but
all installation steps can be done at the command line with the exception of
configuring Docker Desktop. It may be possible to use a headless server if
Docker is already properly configured. The following paragraphs contain the
rationale for choosing this particular build of Windows. If this does not
interest you, skip to the next section.

Docker on Windows supports two different ways to run containers, process
isolation and Hyper-V isolation. Hyper-V isolation is more secure and more
flexible, but it is too slow for our purposes; it essentially spins up a VM for
each container. In order to run process isolation, the build number of the
agent machine and the docker image [need to match][ver-compat]. You can use
`winver` to print the version of Windows from the command line. Windows Server
2016 additionally requires that the **revision number** of the agent machine
and container image match as well. VMs based on a particular revision of
Windows can be hard to find, so we want to use a build later than Server 2016.

The `rustops/crates-build-env-windows` image is built on Azure Pipelines CI
infrastructure, which provides a Windows 1803 host with docker pre-installed
(the latest Windows version available on AppVeyor is Server 2016). Therefore we
use this same build of Windows to do `crater` runs. Note that process isolation
is [disabled on Windows 10 prior to build 1809][win-10], so you **must** use
Windows Server. Additionally, running containers on Windows 10 requires a
Professional or Enterprise license. It's possible to run `crater` on other
builds of Windows as long as you understand these constraints, but you'll need
to build your own `crates-build-env-windows` image based on a compatible
version, as the `rustops` one is based on Windows 1803.

[ver-compat]: https://docs.microsoft.com/en-us/virtualization/windowscontainers/deploy-containers/version-compatibility
[win-10]: https://docs.microsoft.com/en-us/virtualization/windowscontainers/about/faq#can-i-run-windows-containers-in-process-isolated-mode-on-windows-10-enterprise-or-professional

## Install [**Docker Desktop for Windows**][docker-desktop]

[docker-desktop]: https://hub.docker.com/editions/community/docker-ce-desktop-windows

- Download and run the [**Docker Desktop for Windows**][docker-desktop] installer (*not* Docker Toolbox):
  ```powershell
  (New-Object System.Net.WebClient).DownloadFile("https://download.docker.com/win/stable/Docker%20for%20Windows%20Installer.exe", ".\DockerInstaller.exe"); `
  .\DockerInstaller.exe
  ```

- Check "Use Windows containers instead of Linux containers" during
  installation. Once installed, double check that Windows containers are indeed
  enabled by right-clicking the Docker icon in the dock.

- Docker will then detect whether the "Containerization" and "Hyper-V" OS
  features are enabled. If they are disabled, Docker will try to enable them
  for you. Alternatively, run the following commands and then reboot.

  ```powershell
  Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -All
  Enable-WindowsOptionalFeature -Online -FeatureName Containers -All
  ```

## Install [`chocolatey`][] and `git`

[`chocolatey`]: https://chocolatey.org/install

Chocolately is a 3rd-party package manager for Windows. We'll use it to install a
version of `git` which can be used directly from `PowerShell`, as well as some
other useful things:

```Powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; iex ((New-Object System.Net.WebClient).DownloadString('https://chocolatey.org/install.ps1'))
```
Then we can install `git`:

```Powershell
choco install git
```

## Install the Visual Studio VC++ tools

We could use the commands from the Dockerfile, but since we already installed `chocolatey`:

```powershell
choco install -y visualstudio2017-workload-vctools
```

## Install rust locally

Follow the normal procedure, but note that `rustup-init.exe` cannot be
[downloaded from the command line using normal Windows
utilities][rustup-download]. Instead, use `curl.exe` (the suffix is important),
which is installed by default on newer versions of Windows or available via
`chocolately`:

```powershell
curl.exe -o rustup-init.exe https://win.rustup.rs/x86_64
.\rustup-init.exe
```

[rustup-download]: https://github.com/rust-lang/rustup.rs/issues/829
["Trusted Sites"]: https://www.itg.ias.edu/content/how-add-trusted-sites-internet-explorer

Disabling "Real-time Protection" in "Settings > Update & Security > Windows
Defender" may speed up [the installation of `rust-docs`][rust-docs].

[rust-docs]: https://github.com/rust-lang/rustup.rs/issues/1540

## Build crater

The next step is to download and build `crater` just like on a [Linux
agent](./agent-machine-setup.md):

```powershell
git clone https://github.com/rust-lang/crater
cd crater
cargo build --release

# This is going to take a while to complete
cargo run --release -- prepare-local
```

Remember to run `cargo run -- create-lists` before running the tests.
