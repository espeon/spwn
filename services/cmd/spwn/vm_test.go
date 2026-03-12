package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"

	"github.com/spwn/spwn/services/client"
)

var testVM = client.VM{
	ID:        "550e8400-e29b-41d4-a716-446655440000",
	Name:      "epic vm",
	Status:    "running",
	Subdomain: "epic-vm.nat",
}

// newTestServer returns an httptest.Server that mimics /api/vms behaviour.
// It responds to:
//
//	GET /api/vms?name=<n>      — match by name
//	GET /api/vms?subdomain=<s> — match by full or bare subdomain
//	GET /api/vms/<uuid>        — match by ID
func newTestServer(t *testing.T, vm client.VM) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")

		// single VM by ID
		if r.URL.Path == "/api/vms/"+vm.ID {
			json.NewEncoder(w).Encode(vm)
			return
		}

		if r.URL.Path == "/api/vms" {
			q := r.URL.Query()

			if name := q.Get("name"); name != "" {
				if name == vm.Name {
					json.NewEncoder(w).Encode([]client.VM{vm})
				} else {
					json.NewEncoder(w).Encode([]client.VM{})
				}
				return
			}

			if sub := q.Get("subdomain"); sub != "" {
				// match full subdomain ("epic-vm.nat") or bare prefix ("epic-vm")
				if sub == vm.Subdomain || sub == bareSubdomain(vm.Subdomain) {
					json.NewEncoder(w).Encode([]client.VM{vm})
				} else {
					json.NewEncoder(w).Encode([]client.VM{})
				}
				return
			}

			json.NewEncoder(w).Encode([]client.VM{})
			return
		}

		http.NotFound(w, r)
	}))
}

// bareSubdomain strips the ".username" suffix from a full subdomain.
func bareSubdomain(sub string) string {
	for i := len(sub) - 1; i >= 0; i-- {
		if sub[i] == '.' {
			return sub[:i]
		}
	}
	return sub
}

func clientForServer(srv *httptest.Server) *client.Client {
	return client.New(srv.URL, "test-token")
}

func TestResolveVM_ByName(t *testing.T) {
	srv := newTestServer(t, testVM)
	defer srv.Close()

	got, err := resolveVM(clientForServer(srv), "epic vm")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got.ID != testVM.ID {
		t.Errorf("got ID %q, want %q", got.ID, testVM.ID)
	}
}

func TestResolveVM_ByFullSubdomain(t *testing.T) {
	srv := newTestServer(t, testVM)
	defer srv.Close()

	got, err := resolveVM(clientForServer(srv), "epic-vm.nat")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got.ID != testVM.ID {
		t.Errorf("got ID %q, want %q", got.ID, testVM.ID)
	}
}

func TestResolveVM_ByBareSubdomain(t *testing.T) {
	srv := newTestServer(t, testVM)
	defer srv.Close()

	// "epic-vm" has no dot and no exact name match — should fall back to subdomain lookup
	got, err := resolveVM(clientForServer(srv), "epic-vm")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got.ID != testVM.ID {
		t.Errorf("got ID %q, want %q", got.ID, testVM.ID)
	}
}

func TestResolveVM_ByID(t *testing.T) {
	srv := newTestServer(t, testVM)
	defer srv.Close()

	got, err := resolveVM(clientForServer(srv), testVM.ID)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got.ID != testVM.ID {
		t.Errorf("got ID %q, want %q", got.ID, testVM.ID)
	}
}

func TestResolveVM_NotFound(t *testing.T) {
	srv := newTestServer(t, testVM)
	defer srv.Close()

	_, err := resolveVM(clientForServer(srv), "ghost-vm")
	if err == nil {
		t.Fatal("expected error for unknown VM, got nil")
	}
}

func TestResolveVM_FullSubdomainNotFound(t *testing.T) {
	srv := newTestServer(t, testVM)
	defer srv.Close()

	_, err := resolveVM(clientForServer(srv), "ghost-vm.nat")
	if err == nil {
		t.Fatal("expected error for unknown subdomain, got nil")
	}
}

// TestLooksLikeID verifies that only well-formed UUIDs are treated as IDs.
func TestLooksLikeID(t *testing.T) {
	cases := []struct {
		input string
		want  bool
	}{
		{"550e8400-e29b-41d4-a716-446655440000", true},
		{"epic vm", false},
		{"epic-vm.nat", false},
		{"epic-vm", false},
		// wrong length
		{"550e8400-e29b-41d4-a716", false},
		// right length, wrong dash count
		{"550e8400xe29bx41d4xa716x446655440000", false},
	}
	for _, tc := range cases {
		got := looksLikeID(tc.input)
		if got != tc.want {
			t.Errorf("looksLikeID(%q) = %v, want %v", tc.input, got, tc.want)
		}
	}
}

// TestGetVMBySubdomainRequest verifies the client sends the correct query param.
func TestGetVMBySubdomainRequest(t *testing.T) {
	var capturedQuery url.Values
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		capturedQuery = r.URL.Query()
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode([]client.VM{})
	}))
	defer srv.Close()

	client.New(srv.URL, "tok").GetVMBySubdomain("epic-vm.nat")

	if got := capturedQuery.Get("subdomain"); got != "epic-vm.nat" {
		t.Errorf("subdomain query param = %q, want %q", got, "epic-vm.nat")
	}
	if name := capturedQuery.Get("name"); name != "" {
		t.Errorf("unexpected name param %q in subdomain request", name)
	}
}
