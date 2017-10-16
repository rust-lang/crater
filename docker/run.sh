#!/bin/bash

usermod -u "$USER_ID" crater
exec su crater -c "/run2.sh $CMD"
