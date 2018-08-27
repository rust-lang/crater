# Setting up a new Crater agent machine

This document contains the steps to configure a new Crater agent machine from
scratch, from an Amazon Linux image. To get a new one ask in `#rust-infra`.

The machine type we're currently using is:

* Instance: AWS `m3.2xlarge`
* Storage: 1.5 Tb
* OS: Amazon Linux

Once they tell you the IP of the machine, ssh with the `ec2-user` user into it
from the bastion server, and execute these commands:

```
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
sudo yum install git htop docker gcc cmake openssl-devel
sudo yum install https://dl.fedoraproject.org/pub/epel/epel-release-latest-7.noarch.rpm
sudo yum install --enablerepo=epel byobu
sudo service docker start
sudo usermod -a -G docker ec2-user

# Configure byobu to use Ctrl+Z instead of Ctrl+A
cat > ~/.byobu/keybindings.tmux << EOF
unbind-key -n C-a
unbind-key -n C-z
set -g prefix ^Z
set -g prefix2 ^Z
bind z send-prefix
EOF
```

Log out of the machine and in again. Then, execute these commands:

```
git clone https://github.com/rust-lang-nursery/crater
cd crater
cargo build --release

# This is going to take a while to complete
cargo run --release -- prepare-local
```
