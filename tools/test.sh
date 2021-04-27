#!/bin/bash

deps=$(cat Cargo.toml | grep "{ path" | cut -f1 -d' ')

for dep in $deps
do
	cargo test -p $dep
done 
