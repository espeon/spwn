package client

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

type Config struct {
	APIURL        string `json:"api_url"`
	DefaultVcores int    `json:"default_vcores"`
	DefaultMemMb  int    `json:"default_memory_mb"`
}

type Credentials struct {
	Token string `json:"token"`
}

func configDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".config", "spwn"), nil
}

func LoadConfig() (Config, error) {
	dir, err := configDir()
	if err != nil {
		return defaultConfig(), nil
	}
	data, err := os.ReadFile(filepath.Join(dir, "config.json"))
	if os.IsNotExist(err) {
		return defaultConfig(), nil
	}
	if err != nil {
		return defaultConfig(), nil
	}
	var c Config
	if err := json.Unmarshal(data, &c); err != nil {
		return defaultConfig(), nil
	}
	if c.APIURL == "" {
		c.APIURL = defaultConfig().APIURL
	}
	return c, nil
}

func defaultConfig() Config {
	return Config{
		APIURL:        "https://spwn.run",
		DefaultVcores: 2,
		DefaultMemMb:  512,
	}
}

func SaveConfig(c Config) error {
	dir, err := configDir()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	data, err := json.MarshalIndent(c, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(dir, "config.json"), data, 0600)
}

func LoadCredentials() (Credentials, error) {
	dir, err := configDir()
	if err != nil {
		return Credentials{}, fmt.Errorf("no credentials found: run 'spwn login'")
	}
	data, err := os.ReadFile(filepath.Join(dir, "credentials"))
	if err != nil {
		return Credentials{}, fmt.Errorf("not logged in: run 'spwn login'")
	}
	var creds Credentials
	if err := json.Unmarshal(data, &creds); err != nil {
		return Credentials{}, fmt.Errorf("corrupt credentials: run 'spwn login'")
	}
	return creds, nil
}

func SaveCredentials(creds Credentials) error {
	dir, err := configDir()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	data, err := json.MarshalIndent(creds, "", "  ")
	if err != nil {
		return err
	}
	path := filepath.Join(dir, "credentials")
	if err := os.WriteFile(path, data, 0600); err != nil {
		return err
	}
	return nil
}

func ClearCredentials() error {
	dir, err := configDir()
	if err != nil {
		return nil
	}
	path := filepath.Join(dir, "credentials")
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}

func NewAuthedClient() (*Client, error) {
	cfg, err := LoadConfig()
	if err != nil {
		return nil, err
	}
	creds, err := LoadCredentials()
	if err != nil {
		return nil, err
	}
	return New(cfg.APIURL, creds.Token), nil
}
