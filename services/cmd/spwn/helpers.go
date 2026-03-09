package main

import "github.com/spwn/spwn/services/client"

func loadConfigOrDefault() (client.Config, error) {
	return client.LoadConfig()
}

func saveConfig(cfg client.Config) error {
	return client.SaveConfig(cfg)
}
