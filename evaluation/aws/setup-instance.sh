#!/bin/bash

yum -y install cmake gcc m4 gmp gmp-devel mpfr mpfr-devel libmpc libmpc-devel dstat

wget -O /bin/randchaind https://randchain-dev.s3-us-west-1.amazonaws.com/randchaind
chmod +x /bin/randchaind
chmod 777 /bin/randchaind
chown ec2-user /bin/randchaind

echo '#!/bin/bash' >> /home/ec2-user/main.sh
echo 'dstat --integer --noupdate -T -n --tcp --cpu --mem --output /home/ec2-user/stats.csv 1 &> /dev/null &' >> /home/ec2-user/main.sh
echo 'nohup randchaind --verification-level none --blocktime $1 --num-nodes $2 --num-miners $3 -p $4 > /home/ec2-user/main.log 2>&1 &' >> /home/ec2-user/main.sh
chmod +x /home/ec2-user/main.sh
chmod 777 /home/ec2-user/main.sh
chown ec2-user /home/ec2-user/main.sh
