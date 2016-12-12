#!/bin/bash

usermod -u $USER_ID cargobomb
su cargobomb -c "/run2.sh $CMD"
