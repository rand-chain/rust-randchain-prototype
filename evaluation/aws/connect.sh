#!/bin/bash

ssh -i "~/.ssh/randchain.pem" -oStrictHostKeyChecking=accept-new -l ec2-user $1
