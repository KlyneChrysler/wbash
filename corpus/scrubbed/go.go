package main

import (
	"encoding/json"
	"fmt"
	"os"
	"sort"
)

type Item struct {
	Name  string `json:"name"`
	Count int    `json:"count"`
}

func load(path string) ([]Item, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", path, err)
	}
	var items []Item
	if err := json.Unmarshal(data, &items); err != nil {
		return nil, fmt.Errorf("decode %s: %w", path, err)
	}
	sort.Slice(items, func(i, j int) bool { return items[i].Count > items[j].Count })
	return items, nil
}

func main() {
	items, err := load("items.json")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	for _, item := range items {
		fmt.Printf("%s %d\n", item.Name, item.Count)
	}
}
