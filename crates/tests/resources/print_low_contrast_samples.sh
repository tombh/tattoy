#!/bin/bash

function bg() {
	printf "\033[48;2;%d;%d;%dm" $1 $2 $3
}

function fg() {
	printf "\033[38;2;%d;%d;%dm" $1 $2 $3
}

fg 123 123 123
bg 140 150 120

echo middle

fg 40 0 0
bg 0 0 0

echo dark

fg 200 200 200
bg 180 200 200

echo light
