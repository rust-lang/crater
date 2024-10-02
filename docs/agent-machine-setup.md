# Setting up a new Crater agent machine

This document contains the steps to configure a new Crater agent machine from
scratch, from an Amazon Linux image. To get a new one ask in
[t-infra](https://rust-lang.zulipchat.com/#narrow/stream/242791-t-infra).

The machine type we're currently using is:

* Instance: AWS `c5.2xlarge`
* Storage: 2 Tb
* OS: Amazon Linux 2

Once they tell you the IP of the machine, ssh with the `ec2-user` user into it
from the bastion server, and execute these commands:

```
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
sudo yum install git htop docker gcc cmake openssl-devel
sudo yum install https://dl.fedoraproject.org/pub/epel/epel-release-latest-7.noarch.rpm
sudo yum install --enablerepo=epel byobu
sudo systemctl start docker
sudo systemctl enable docker
sudo usermod -a -G docker ec2-user

# Configure byobu to use Ctrl+Z instead of Ctrl+A
mkdir ~/.byobu
cat > ~/.byobu/keybindings.tmux << EOF
unbind-key -n C-a
unbind-key -n C-z
set -g prefix ^Z
set -g prefix2 ^Z
bind z send-prefix
EOF
```

Log out of the machine. From the bastion, copy the `~/.aws/credentials` file
from an existing agent into the new one, and then log into it again and
execute:

```
git clone https://github.com/rust-lang/crater
cd crater
cargo build --release

# This is going to take a while to complete
cargo run --release -- prepare-local
```
