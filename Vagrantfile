Vagrant.configure("2") do |config|
  config.vm.box = "ubuntu/xenial64"

  # Disable the default syncing
  config.vm.synced_folder ".", "/vagrant", disabled: true
  # Instead sync the actual worker folder and the lib if it's around
  config.vm.synced_folder ".", "/home/vagrant/v9_worker"
  config.vm.synced_folder "../v9_lib", "/home/vagrant/v9_lib"

  config.vm.network "forwarded_port", guest: 8082, host: 8082

  config.vm.provider :virtualbox do |vb|
    vb.customize ["modifyvm", :id, "--ioapic", "on"] 
    vb.customize ["modifyvm", :id, "--memory", "4096"]
    vb.customize ["modifyvm", :id, "--cpus", "2"]   
  end

  config.vm.provision "shell", privileged: false, inline: <<-SHELL
    sudo apt-get update
    sudo apt-get upgrade -y
    sudo apt-get autoremove -y
    sudo apt-get install python3 python3-dev python3-pip -y
    sudo apt-get install vim git nasm tmux -y
    sudo apt-get install httpie docker.io -y
    curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain stable -y
    export PATH="$HOME/.cargo/bin:$PATH"
    rustup component add rust-src
    echo "export PATH="$HOME/.cargo/bin:$PATH"" >> $HOME/.bashrc
  SHELL
end

