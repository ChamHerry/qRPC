package main

import (
	"context"
	"fmt"
	"log"

	qrpc "github.com/ChamHerry/qRPC/go"
)

func main() {
	client := qrpc.NewClient("127.0.0.1:7000")
	response, err := client.Unary(context.Background(), "/demo.Echo/Unary", []byte("hello"))
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("go echo: %s\n", response)
}
