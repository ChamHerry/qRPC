package qrpc

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestUnaryUsesRustCABI(t *testing.T) {
	client := NewClient("127.0.0.1:7000")
	got, err := client.Unary(context.Background(), "/demo.Echo/Unary", []byte("ping"))
	if err != nil {
		t.Fatalf("unary failed: %v", err)
	}
	if string(got) != "ping" {
		t.Fatalf("unexpected response: %q", got)
	}
}

func TestStreamUsesChannelBridge(t *testing.T) {
	client := NewClient("127.0.0.1:7000")
	stream, err := client.NewStream(context.Background(), "/demo.Echo/Chat")
	if err != nil {
		t.Fatalf("new stream failed: %v", err)
	}
	defer stream.destroy()

	for _, payload := range []string{"a", "b"} {
		if err := stream.Send([]byte(payload)); err != nil {
			t.Fatalf("send failed: %v", err)
		}
		got, err := stream.Recv()
		if err != nil {
			t.Fatalf("recv failed: %v", err)
		}
		if string(got) != payload {
			t.Fatalf("unexpected stream response: %q", got)
		}
	}
	if err := stream.CloseSend(); err != nil {
		t.Fatalf("close send failed: %v", err)
	}
}

func TestContextCancel(t *testing.T) {
	client := NewClient("127.0.0.1:7000")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := client.Unary(ctx, "/demo.Echo/Unary", []byte("ping"))
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("expected context canceled, got %v", err)
	}

	stream, err := client.NewStream(context.Background(), "/demo.Echo/Chat")
	if err != nil {
		t.Fatalf("new stream failed: %v", err)
	}
	defer stream.destroy()
	ctx, cancel = context.WithTimeout(context.Background(), time.Nanosecond)
	defer cancel()
	stream.ctx = ctx
	time.Sleep(time.Millisecond)
	if err := stream.Send([]byte("late")); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected deadline exceeded, got %v", err)
	}
}
