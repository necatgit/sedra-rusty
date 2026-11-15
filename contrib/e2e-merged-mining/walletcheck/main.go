// walletcheck calls the sedrawallet daemon the way Miningcore's payout does: GetBalance, then Send with the wallet
// password (the daemon signs), use-existing-change-address on, not send-all. Prints the balance and the transaction ids.
//
// Usage: walletcheck <daemon host:port> <password> <to address> <amount in SDR, 0 = balance only>
package main

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/sedracoin/sedrad/cmd/sedrawallet/daemon/pb"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

const sompiPerSedra = 100_000_000

func main() {
	if len(os.Args) != 5 {
		fmt.Fprintln(os.Stderr, "usage: walletcheck <daemon> <password> <to> <amount>")
		os.Exit(2)
	}
	amount, err := strconv.ParseFloat(os.Args[4], 64)
	if err != nil {
		panic(err)
	}

	conn, err := grpc.Dial(os.Args[1], grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		panic(err)
	}
	defer conn.Close()
	client := pb.NewSedrawalletdClient(conn)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	balance, err := client.GetBalance(ctx, &pb.GetBalanceRequest{})
	if err != nil {
		fmt.Println("GetBalance FAILED:", err)
		os.Exit(1)
	}
	fmt.Printf("balance available=%d pending=%d sompi\n", balance.Available, balance.Pending)
	if amount == 0 {
		return
	}

	sent, err := client.Send(ctx, &pb.SendRequest{
		ToAddress:                os.Args[3],
		Amount:                   uint64(amount * sompiPerSedra),
		Password:                 os.Args[2],
		UseExistingChangeAddress: true,
		IsSendAll:                false,
	})
	if err != nil {
		fmt.Println("Send FAILED:", err)
		os.Exit(1)
	}
	fmt.Println("sent txids:", sent.TxIDs)
}
