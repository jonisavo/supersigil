#!/bin/sh

rsync -avz --delete dist/ joni@savolainen.io:/var/www/future/supersigil
