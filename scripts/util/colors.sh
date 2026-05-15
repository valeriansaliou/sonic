##
#  Some variables to make coloring easier.
#
#  Copyright: 2026, Rémi Bardon <remi@remibardon.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

# Reset all attributes
A_RESET='\033[0m'

# Color
C_RED='\033[31m'
C_GREEN='\033[32m'
C_YELLOW='\033[33m'
C_BLUE='\033[34m'
C_RESET='\033[39m'

# Intensity
I_BOLD='\033[1m'
I_DIM='\033[2m'
I_RESET='\033[22m'

# Style
S_UNDERLINE='\033[4m'
S_UNDERLINE_OFF='\033[24m'

fg_red() {
  printf "${C_RED}%s${C_RESET}" "$*"
}
fg_green() {
  printf "${C_GREEN}%s${C_RESET}" "$*"
}
fg_yellow() {
  printf "${C_YELLOW}%s${C_RESET}" "$*"
}
fg_blue() {
  printf "${C_BLUE}%s${C_RESET}" "$*"
}
