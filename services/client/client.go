package client

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"time"
)

type Client struct {
	baseURL string
	token   string
	http    *http.Client
}

func New(baseURL, token string) *Client {
	return &Client{
		baseURL: baseURL,
		token:   token,
		http:    &http.Client{Timeout: 30 * time.Second},
	}
}

type APIError struct {
	Status  int
	Message string
}

func (e *APIError) Error() string {
	return fmt.Sprintf("API error %d: %s", e.Status, e.Message)
}

func (c *Client) do(method, path string, body any) (*http.Response, error) {
	var bodyReader io.Reader
	if body != nil {
		b, err := json.Marshal(body)
		if err != nil {
			return nil, fmt.Errorf("marshal request: %w", err)
		}
		bodyReader = bytes.NewReader(b)
	}

	req, err := http.NewRequest(method, c.baseURL+path, bodyReader)
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}

	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if c.token != "" {
		req.Header.Set("Authorization", "Bearer "+c.token)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("request %s %s: %w", method, path, err)
	}
	return resp, nil
}

func decode[T any](resp *http.Response) (T, error) {
	defer resp.Body.Close()
	var zero T
	if resp.StatusCode >= 400 {
		msg, _ := io.ReadAll(resp.Body)
		return zero, &APIError{Status: resp.StatusCode, Message: string(msg)}
	}
	if err := json.NewDecoder(resp.Body).Decode(&zero); err != nil {
		return zero, fmt.Errorf("decode response: %w", err)
	}
	return zero, nil
}

func drain(resp *http.Response) error {
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		msg, _ := io.ReadAll(resp.Body)
		return &APIError{Status: resp.StatusCode, Message: string(msg)}
	}
	return nil
}

// ── Auth ──────────────────────────────────────────────────────────────────────

type CLIInitResponse struct {
	Code       string `json:"code"`
	BrowserURL string `json:"browser_url"`
	ExpiresIn  int    `json:"expires_in"`
}

type CLIPollResponse struct {
	Status string  `json:"status"`
	Token  *string `json:"token,omitempty"`
}

func (c *Client) CLIInit(baseURL string) (CLIInitResponse, error) {
	path := "/auth/cli/init"
	if baseURL != "" {
		path += "?base_url=" + url.QueryEscape(baseURL)
	}
	resp, err := c.do("POST", path, nil)
	if err != nil {
		return CLIInitResponse{}, err
	}
	return decode[CLIInitResponse](resp)
}

func (c *Client) CLIPoll(code string) (CLIPollResponse, error) {
	resp, err := c.do("GET", "/auth/cli/poll?code="+url.QueryEscape(code), nil)
	if err != nil {
		return CLIPollResponse{}, err
	}
	return decode[CLIPollResponse](resp)
}

// ── Account ───────────────────────────────────────────────────────────────────

type Account struct {
	ID          string `json:"id"`
	Email       string `json:"email"`
	Username    string `json:"username"`
	DisplayName string `json:"display_name"`
	Theme       string `json:"theme"`
	VmLimit     int    `json:"vm_limit"`
	VcpuLimit   int64  `json:"vcpu_limit"`
	MemLimitMb  int    `json:"mem_limit_mb"`
}

func (c *Client) Me() (Account, error) {
	resp, err := c.do("GET", "/auth/me", nil)
	if err != nil {
		return Account{}, err
	}
	return decode[Account](resp)
}

// ── VMs ───────────────────────────────────────────────────────────────────────

type VM struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Status      string `json:"status"`
	Subdomain   string `json:"subdomain"`
	Vcpus       int64  `json:"vcpus"`
	MemoryMb    int    `json:"memory_mb"`
	IPAddress   string `json:"ip_address"`
	ExposedPort int    `json:"exposed_port"`
	Image       string `json:"image"`
}

type CreateVMRequest struct {
	Name        string `json:"name"`
	Image       string `json:"image,omitempty"`
	Vcpus       int64  `json:"vcpus,omitempty"`
	MemoryMb    int    `json:"memory_mb,omitempty"`
	ExposedPort int    `json:"exposed_port,omitempty"`
}

type PatchVMRequest struct {
	Name        *string `json:"name,omitempty"`
	ExposedPort *int    `json:"exposed_port,omitempty"`
}

func (c *Client) ListVMs() ([]VM, error) {
	resp, err := c.do("GET", "/api/vms", nil)
	if err != nil {
		return nil, err
	}
	return decode[[]VM](resp)
}

func (c *Client) GetVM(id string) (VM, error) {
	resp, err := c.do("GET", "/api/vms/"+id, nil)
	if err != nil {
		return VM{}, err
	}
	return decode[VM](resp)
}

func (c *Client) GetVMByName(name string) ([]VM, error) {
	resp, err := c.do("GET", "/api/vms?name="+url.QueryEscape(name), nil)
	if err != nil {
		return nil, err
	}
	return decode[[]VM](resp)
}

func (c *Client) GetVMBySubdomain(subdomain string) ([]VM, error) {
	resp, err := c.do("GET", "/api/vms?subdomain="+url.QueryEscape(subdomain), nil)
	if err != nil {
		return nil, err
	}
	return decode[[]VM](resp)
}

func (c *Client) CreateVM(req CreateVMRequest) (VM, error) {
	resp, err := c.do("POST", "/api/vms", req)
	if err != nil {
		return VM{}, err
	}
	return decode[VM](resp)
}

func (c *Client) StartVM(id string) error {
	resp, err := c.do("POST", "/api/vms/"+id+"/start", nil)
	if err != nil {
		return err
	}
	return drain(resp)
}

func (c *Client) StopVM(id string) error {
	resp, err := c.do("POST", "/api/vms/"+id+"/stop", nil)
	if err != nil {
		return err
	}
	return drain(resp)
}

func (c *Client) DeleteVM(id string) error {
	resp, err := c.do("DELETE", "/api/vms/"+id, nil)
	if err != nil {
		return err
	}
	return drain(resp)
}

func (c *Client) PatchVM(id string, req PatchVMRequest) (VM, error) {
	resp, err := c.do("PATCH", "/api/vms/"+id, req)
	if err != nil {
		return VM{}, err
	}
	return decode[VM](resp)
}

type CloneVMRequest struct {
	Name          string `json:"name"`
	IncludeMemory bool   `json:"include_memory"`
}

func (c *Client) CloneVM(id string, req CloneVMRequest) (VM, error) {
	resp, err := c.do("POST", "/api/vms/"+id+"/clone", req)
	if err != nil {
		return VM{}, err
	}
	return decode[VM](resp)
}

// ── VM Events ─────────────────────────────────────────────────────────────────

type VMEvent struct {
	ID        int64   `json:"id"`
	VmID      string  `json:"vm_id"`
	Event     string  `json:"event"`
	Metadata  *string `json:"metadata"`
	CreatedAt int64   `json:"created_at"`
}

func (c *Client) ListVMEvents(vmID string, limit int, before *int64) ([]VMEvent, error) {
	path := fmt.Sprintf("/api/vms/%s/events?limit=%d", vmID, limit)
	if before != nil {
		path += fmt.Sprintf("&before=%d", *before)
	}
	resp, err := c.do("GET", path, nil)
	if err != nil {
		return nil, err
	}
	return decode[[]VMEvent](resp)
}

// ── Snapshots ─────────────────────────────────────────────────────────────────

type Snapshot struct {
	ID        string  `json:"id"`
	VmID      string  `json:"vm_id"`
	Label     *string `json:"label"`
	SizeBytes int64   `json:"size_bytes"`
	CreatedAt int64   `json:"created_at"`
}

func (c *Client) ListSnapshots(vmID string) ([]Snapshot, error) {
	resp, err := c.do("GET", "/api/vms/"+vmID+"/snapshots", nil)
	if err != nil {
		return nil, err
	}
	return decode[[]Snapshot](resp)
}

func (c *Client) TakeSnapshot(vmID string, label *string) (Snapshot, error) {
	var body any
	if label != nil {
		body = map[string]string{"label": *label}
	}
	resp, err := c.do("POST", "/api/vms/"+vmID+"/snapshot", body)
	if err != nil {
		return Snapshot{}, err
	}
	return decode[Snapshot](resp)
}

func (c *Client) DeleteSnapshot(vmID, snapID string) error {
	resp, err := c.do("DELETE", "/api/vms/"+vmID+"/snapshots/"+snapID, nil)
	if err != nil {
		return err
	}
	return drain(resp)
}

func (c *Client) RestoreSnapshot(vmID, snapID string) error {
	resp, err := c.do("POST", "/api/vms/"+vmID+"/restore/"+snapID, nil)
	if err != nil {
		return err
	}
	return drain(resp)
}

// ── SSH keys ──────────────────────────────────────────────────────────────────

type SSHKey struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Fingerprint string `json:"fingerprint"`
	CreatedAt   int64  `json:"created_at"`
}

func (c *Client) ListSSHKeys() ([]SSHKey, error) {
	resp, err := c.do("GET", "/api/account/keys", nil)
	if err != nil {
		return nil, err
	}
	return decode[[]SSHKey](resp)
}

func (c *Client) AddSSHKey(name, publicKey string) (SSHKey, error) {
	resp, err := c.do("POST", "/api/account/keys", map[string]string{
		"name":       name,
		"public_key": publicKey,
	})
	if err != nil {
		return SSHKey{}, err
	}
	return decode[SSHKey](resp)
}

func (c *Client) DeleteSSHKey(id string) error {
	resp, err := c.do("DELETE", "/api/account/keys/"+id, nil)
	if err != nil {
		return err
	}
	return drain(resp)
}

type ServerConfig struct {
	SSHGatewayAddr string `json:"ssh_gateway_addr"`
}

func (c *Client) GetServerConfig() (ServerConfig, error) {
	resp, err := c.do("GET", "/api/config", nil)
	if err != nil {
		return ServerConfig{}, err
	}
	return decode[ServerConfig](resp)
}
