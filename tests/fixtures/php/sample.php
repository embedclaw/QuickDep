<?php

namespace Acme\Sample;

use Acme\Shared\Helper as SharedHelper;
use Acme\Shared\Logger;
use function Acme\Shared\format_name;

interface Greeter
{
    public function greet(string $name): string;
}

class BaseService
{
}

class UserService extends BaseService implements Greeter
{
    private string $name = '';

    public function greet(string $name): string
    {
        $helper = new SharedHelper();
        Logger::write(format_name($name));
        return $helper->decorate($name);
    }
}
