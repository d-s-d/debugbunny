# 🐰 debugbunny 🛠️

A small toolbox to scrape HTTP-endpoints and commands on a regular basis and
publish the results as log messages.

## Overview

Let's say you operate a service and you want to assess part of the system state
after the fact — debugbunny is here to help you. It currently allows you to
specify _scrape targets_ that are either HTTP-endpoints or shell commands. Each
scrape target is scraped at the chosen interval. The debugbunny-crate contains
tooling to turn the results into logable messages (see below).

