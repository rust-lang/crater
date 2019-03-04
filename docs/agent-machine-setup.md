# Setting up a new Crater agent machine

This document contains the steps to configure a new Crater agent machine from
scratch, from an Amazon Linux image. To get a new one ask [in
`#infra`][discord].

The machine type we're currently using is:

* Instance: AWS `c5.2xlarge`
* Storage: 4 Tb
* OS: Amazon Linux 2

You might need to create a smaller root partition and attach the 4 Tb disk on
another directory since we had problems mounting partitions bigger than 2 Tb on
the root directory.

Once they tell you the IP of the machine, ssh with the `ec2-user` user into it
from the bastion server, and execute these commands:

```
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
sudo yum install git htop docker gcc cmake openssl-devel
sudo systemctl start docker
sudo systemctl enable docker
sudo usermod -a -G docker ec2-user
```

Log out of the machine and log in again, executing these commands:

```
git clone https://github.com/rust-lang-nursery/crater
cd crater
cargo build --release

cat > /etc/systemd/system/crater-agent.service << EOF
[Unit]
Description = Crater agent

[Service]
ExecStart=/home/ec2-user/crater/target/release/crater agent https://crater.rust-lang.org <TOKEN> --threads 8
Restart=on-failure
User=ec2-user
Group=ec2-user
WorkingDirectory=/home/ec2-user/crater

[Install]
WantedBy=multi-user.target
EOF

systemctl enable crater-agent
systemctl start crater-agent
```

[discord]: https://discord.gg/AxXmxzN
