Vagrant.configure("2") do |config|
  config.vm.box = "ubuntu/xenial64"

  # Disable the default syncing
  config.vm.synced_folder ".", "/vagrant", disabled: true
  # Instead sync the actual worker folder and the lib if it's around
  config.vm.synced_folder ".", "/home/vagrant/v9_worker"
  config.vm.synced_folder "../v9_lib", "/home/vagrant/v9_lib"
end

